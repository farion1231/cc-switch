# Routing Auto-Toggle — 设计与实施文档

> 分支：`feat/routing-auto-toggle`
> 角色分工：SubAgent 实现 + 写测试；主 Agent 只做 Review + 卡测试覆盖；人工冒烟。

## 1. 要解决的问题

| 场景 | 当前行为 | 期望行为 |
|---|---|---|
| 启用「需要路由」的 provider（如 Codex→DeepSeek），但当前 app 路由没开 | soft `toast.warning`，仍照常切换 → 静默失败 | 弹「开启本地路由并启用」确认对话框 |
| 路由开着时切到官方 provider（`category==="official"`） | 主按钮 disabled +「已拦截」，须手动去设置关路由 | （opt-in 后）弹「关闭本地路由并切换」确认对话框 |

根因：provider 对路由的硬依赖/不兼容已通过卡片徽章告知用户，但切换动作本身不做衔接。

## 2. 关键事实（已核实）

- **接管是 per-App 的**：`set_takeover_for_app(appType, enabled)`（`services/proxy.rs:535`）只改写该 app 的 Live 配置，关闭时有 `any_enabled` 守卫——只有最后一个 app 关闭才停共享代理服务。**绝不碰全局「代理服务」总开关。**
- **`isProxyTakeover` 已是 per-app**：`App.tsx:298` = `isProxyRunning && isCurrentAppTakeoverActive`，其中 `isCurrentAppTakeoverActive = takeoverStatus?.[activeApp]`，传入 `useProviderActions` 第三参，`switchProvider` 内可直接用。
- **切换唯一前端入口**：`useProviderActions.switchProvider`（`useProviderActions.ts:151`），经 `App.tsx` 传给 `ProviderList` 的 `onSwitch`。**托盘切换是后端旁路（`tray.rs:handle_provider_click`），本功能不覆盖**（见 §决策 Q6）。
- **官方风险已有硬阻断**：`switchProvider:220-229`（`isProxyTakeover && official` → `toast.error` + return）；按钮层 `ProviderActions.tsx:177-185`（`isOfficialBlockedByProxy` → disabled +「已拦截」）。
- **现成「切换前确认」范式**：`ProviderList` 的 streamCheck（`showStreamCheckConfirm` + `pendingTestProvider` + `handleStreamCheckConfirm`，确认后写 settings flag 再执行）。本功能照搬。
- **现成确认控件**：`src/components/ConfirmDialog.tsx`（受控、`variant=info|destructive`、`message` 支持 `\n`）。无 checkbox 能力，需扩展。
- **settings 落地范式**：`AppSettings`（`settings.rs:221`）顶层 typed bool（如 `enable_local_proxy`），前端 `SettingsFormState`（`useSettings`）+ `onAutoSave`。

## 3. 决策记录（Q1–Q11）

- **Q1/Q2 正向触发** = `needsRouting && !isProxyTakeover`。`needsRouting` 以 `switchProvider` 现有 `proxyRequiredReason` 判定集合为真值来源（覆盖 Codex chat/responses、Claude 非 anthropic、Copilot、fullUrl、Claude Desktop proxy），抽成纯函数 **`getProxyRequirement`**，剥掉 `!isProxyRunning` 前置；**卡片徽章也改用它**（覆盖面变广：Copilot/fullUrl 将新显示「需要路由」）。
- **Q3 反向范围** = 仅 `category==="official"`（对齐现有硬阻断与「不支持路由」徽章）。`autoDisableForNoRouting` 关 → 维持硬阻断（「已拦截」）；开 → 弹「关闭路由并切换」。
- **Q4 持久化** = 方案 X，单一状态位。`AppSettings` 加两个顶层 typed bool；弹窗「记住」勾选 = 直接写对应 bool；设置面板两 switch 显示同两值。**无 `*_confirmed` 配套位**。
- **Q5 默认值** = 正向 `auto_enable_for_needs_routing` **默认 false**（首次弹窗 → 勾记住 → 此后静默，与原始 UI 图一致）；反向 `auto_disable_for_no_routing` **默认 false**（opt-in）。
- **Q6 范围** = 仅前端 UI 切换路径（主界面启用按钮 + ProviderList）。托盘旁路不在范围，文档明示。
- **Q7 落点** = guard 放 `ProviderList` 组件，照搬 streamCheck 范式。`useProviderActions.switchProvider` 保持纯粹。
- **Q8 失败处理** = 链式严格 `await`：第 1 步 `set_takeover_for_app` 失败 → `toast.error` 横幅 + 中止，**不执行 `onSwitch`**。「记住」勾选点确认即写 settings（独立于第 1 步成败）。全程 loading 态防重复点击。
- **Q9 按钮 disable** = 改为 `isOfficialBlockedByProxy && !autoDisableForNoRouting` 才 disable。开关开 → 官方按钮恢复可点「启用」→ 点击进 guard 弹「关闭路由并切换」。`autoDisableForNoRouting` 穿 `ProviderList → ProviderCard → ProviderActions`。
- **Q10 设置面板** = 两个 `ToggleRow` 放 `ProxyPanel` 的 `[1] enableLocalProxy` 之后（始终可见），经 `onAutoSave`/`SettingsFormState` 读写。落在 `ProxyTabContent` 的 "Local Proxy" Accordion（`value="proxy"`）。
- **Q11 测试硬性线** = L1 `getProxyRequirement` + L2 `decideSwitchAction`（决策抽纯函数）+ 后端 AppSettings 默认值 Rust 单测；L3 组件交互测试尽力而为，不作打回条件。

## 4. 决策状态机（`decideSwitchAction`）

输入：`{ needsRouting, isProxyTakeover, isOfficial, autoEnable, autoDisable }`
输出：`"direct" | "confirmEnable" | "confirmDisable" | "hardBlock"`

```
isOfficial && isProxyTakeover:
    autoDisable === true  → "confirmDisable"
    else                  → "hardBlock"     // 维持现状「已拦截」
needsRouting && !isProxyTakeover:
    autoEnable === true   → "direct"         // 静默自动开路由+切（记住后）
    else                  → "confirmEnable"  // 首次弹窗
otherwise                 → "direct"         // 无徽章/已满足，直接切
```

- `confirmEnable` 确认 → `await set_takeover_for_app(app, true)`（失败中止）→ `onSwitch`。
- `confirmDisable` 确认 → `await set_takeover_for_app(app, false)`（失败中止）→ `onSwitch`。
- `autoEnable===true` 的 `direct` 分支仍需 `await set_takeover_for_app(app, true)` 再 `onSwitch`（静默，无弹窗）。
- 注意：`hardBlock` 与 `confirmDisable` 的区别由 `autoDisable` 决定，同时驱动 Q9 的按钮 disable 判定，二者必须用同一个开关值，避免「按钮可点但 guard 判 hardBlock」的死结。

## 5. 修改点清单（实现 + 冒烟）

### 后端
1. `src-tauri/src/settings.rs` — `AppSettings` 加 `auto_enable_for_needs_routing: bool`、`auto_disable_for_no_routing: bool`（serde default false）。
2. `src-tauri/src/commands/settings.rs`(+ DAO 若需) — get/save 带新字段。

### 前端纯逻辑
3. 新文件 `src/utils/providerRouting.ts` — `getProxyRequirement(provider, appId): { required: boolean; reason: string | null }`（从 `useProviderActions.ts:168-207` 抽，剥 `!isProxyRunning`）。
4. 新文件 `src/utils/switchDecision.ts` — `decideSwitchAction(input): Action`（§4 状态机）。

### 前端 UI
5. `ProviderCard.tsx` — 徽章改用 `getProxyRequirement`。
6. `ProviderList.tsx` — guard 主逻辑（`pendingSwitchProvider` + `showRoutingConfirm` 方向 state + `handleSwitchWithGuard` + 两个 `ConfirmDialog` 带「记住」checkbox；读 settings 两开关）。
7. `ProviderActions.tsx` — disable 判定改为 `isOfficialBlockedByProxy && !autoDisableForNoRouting`；新增 `autoDisableForNoRouting` prop。
8. `ProxyPanel.tsx` — `[1]` 后加两个 `ToggleRow`。
9. `ProxyTabContent.tsx` — 向 ProxyPanel 传值 + onChange（`onAutoSave`）。
10. `useSettings.ts`(`SettingsFormState`) + `src/lib/schemas/settings.ts` + settings api 类型 — 加两字段。
11. `ConfirmDialog.tsx` — 加可选 `checkbox?: { label; checked; onChange }` prop（不传时行为不变）。
12. i18n `zh.json / en.json / ja.json / zh-TW.json` — 两对话框文案、两开关 label/desc、「记住」、失败 toast。

### 关键穿参链
`autoDisableForNoRouting`：settings → `ProviderList`（已读 settings）→ `ProviderCard` → `ProviderActions`。

## 6. 任务拆分（派给 SubAgent，每个带验收）

- **T1 后端 settings 字段**（修改点 1,2）— 验收：`cargo test` 含 AppSettings 默认值单测（两字段默认 false，缺字段反序列化不报错）；`cargo fmt`/`clippy` clean。
- **T2 `getProxyRequirement` + 测试**（3）— 验收：vitest 覆盖 Codex chat/responses、Claude 非anthropic、Copilot、fullUrl、ClaudeDesktop proxy、official(各 app)、空 config；`pnpm typecheck` pass。
- **T3 `decideSwitchAction` + 测试**（4）— 验收：vitest 覆盖四种输出全部分支 + 默认/opt-in 组合。
- **T4 `ConfirmDialog` checkbox 扩展**（11）— 验收：不传 checkbox 时与原渲染一致（回归）；传入时 checkbox 可控。
- **T5 ProviderCard 徽章切换**（5）— 依赖 T2。验收：徽章覆盖面按 §3-Q2；typecheck pass。
- **T6 ProviderList guard + ProviderActions disable**（6,7）— 依赖 T2/T3/T4。验收：§7 冒烟路径手动可走通；typecheck pass。
- **T7 设置面板两开关 + settings 前端往返**（8,9,10）— 依赖 T1。验收：开关改动重启保持；typecheck pass。
- **T8 i18n**（12）— 验收：四语言无缺 key，无 raw key 漏出。

依赖序：T1‖T2‖T3‖T4 → T5(T2)‖T7(T1) → T6(T2,T3,T4) → T8。

## 7. 人工冒烟最小集

1. **正向首弹+记住**：关 Codex 路由 → 启用 DeepSeek → 弹窗 → 勾记住确认 → 再启用另一需路由 provider 不再弹。
2. **反向 opt-in**：开「自动关闭」→ 路由开下点官方 → 从「已拦截」变可点 → 弹窗 → 确认关+切。
3. **默认安全**：全新状态，官方在路由模式仍「已拦截」（反向默认关）。
4. **回归**：删除 Provider 弹窗照常（ConfirmDialog 未传 checkbox）。
5. **持久化**：开关改动后重启 app 保持。
6. **徽章扩面**：Claude+Copilot / 完整URL provider 新显示「需要路由」。

## 7.5 Review 流程（约定）

每一波实现完成后：
1. **先派独立 Review SubAgent** —— 审数据流 / 控制流的变化、检查代码品味，产出一版独立 Review 报告（不接触主 Agent 的判断）。
2. 主 Agent 基于该报告 + 自己复核，做最终 Review，重点卡：①是否交付对应测试、②测试是否真覆盖验收点、③是否触碰禁区（全局代理总开关 / 托盘 / 新建 DB 字段）。
3. 测试覆盖硬性线见 §3-Q11。

## 8. 明确不做

- 不新建 provider 持久化字段（badge 由 config 派生）。
- 不碰「代理服务」总开关；只用 per-app `set_takeover_for_app`。
- 不覆盖托盘切换（后端旁路，原生菜单无法弹窗）。
- 反向仅覆盖 official；无 `*_confirmed` 第三态。
- OpenCode/OpenClaw/Hermes 不参与（无 takeover、不产生相关徽章，guard 自然短路）。
