# 开发计划：同协议跨模型故障转移（Cross-Model Failover, Anthropic 兼容路线）

> 状态：定稿 v2（已根据决策收敛到「方案 A：备用模型走 Anthropic 兼容接口」）
> 目标：当首选供应商（如 OpenRouter 代理的 Claude）不可用时，自动转移到
> **另一个模型**的供应商（如 DeepSeek / GLM / Qwen 的 Anthropic 兼容端），
> 让 Claude Code 无感降级继续工作。
>
> 关键前提：所有备用供应商都提供 **Anthropic 兼容接口**
> （填 `ANTHROPIC_BASE_URL` + `ANTHROPIC_AUTH_TOKEN` + `ANTHROPIC_MODEL` 即可）。
> 因此**不需要协议桥接、不改路由核心**，是最小风险路线。

---

## 0. 为什么这条路线几乎「零后端改动」

读过源码后确认的事实链：

1. **故障转移在代理接管模式下已完全可用**，候选池由
   `ProviderRouter::select_providers(app_type)`（`src-tauri/src/proxy/provider_router.rs:37`）
   读取 `db.get_failover_queue(app_type)` 产生，按 `sort_index` 排序、跳过已熔断的供应商。
2. **同一个 `app_type=claude` 下可以挂任意多个上游不同的供应商**——队列只要求 `app_type` 相同，
   不要求上游地址/模型相同。
3. **模型名重写已支持任意跨模型映射**：`proxy/model_mapper.rs` 从每个 provider 的
   `settingsConfig.env` 读 `ANTHROPIC_MODEL` / `ANTHROPIC_DEFAULT_{HAIKU,SONNET,OPUS}_MODEL`，
   把入站的 `claude-sonnet-x` 改写成该供应商真正的模型名。其单测已验证
   `claude-sonnet-4-6 → deepseek-v4-pro`（`model_mapper.rs` 测试 291-304 行）。
4. **熔断器天然按供应商隔离**：key = `"{app_type}:{provider_id}"`（`provider_router.rs:70`），
   不同上游的供应商各自独立熔断，互不影响。

> 结论：只要把「DeepSeek 的 Anthropic 兼容端」建成一个 `app_type=claude` 的普通供应商
> 并加入故障转移队列，**现有代码就能跑通跨模型转移**。本计划的工作主要是：
> 把这条路打磨成「用户看得懂、配得对、能验证」的产品体验。

---

## 1. 工作分解

### M1 — 打通并验证基线（最高优先，验证假设）

目的：在改任何 UI 之前，先用纯配置证明现有代码能跨模型转移。

步骤：
1. 启动应用：`pnpm tauri dev`（需 Rust toolchain + Node，见 `.node-version` / `rust-toolchain.toml`）。
2. 在 Claude 分类下建两个供应商：
   - **A（首选）**：OpenRouter，`ANTHROPIC_BASE_URL=https://openrouter.ai/api/v1`（或现用配置），模型 claude-sonnet。
   - **B（备用）**：DeepSeek 的 Anthropic 兼容端，`ANTHROPIC_BASE_URL=<deepseek anthropic 兼容地址>`，
     `ANTHROPIC_AUTH_TOKEN=<key>`，`ANTHROPIC_MODEL=deepseek-chat`（按 DeepSeek 实际模型名填）。
3. 开启 Claude 的代理接管 + `auto_failover_enabled`，把 A、B 加入故障转移队列，A 优先级更高。
4. 制造 A 故障：临时把 A 的 base_url 改成无效地址 / 或断掉其网络，触发连续失败 → 熔断。
5. 观察：流量自动转到 B，Claude Code 仍能正常对话；托盘和前端「当前供应商」切到 B。
6. 恢复 A 后，`reset_circuit_breaker` 路径应自动切回优先级更高的 A（`commands/proxy.rs:320`）。

产出：一份记录「成功/失败/坑」的验证笔记。**若此步通过，后端基本无需改动。**

### M2 — UI：让队列「看得懂是哪个模型」

问题：当前故障转移队列条目只显示供应商名（`FailoverQueueItem`：providerId/providerName/notes/sortIndex），
用户看不出「P2 其实跑的是 deepseek」。

改动：
- `src/components/providers/FailoverPriorityBadge.tsx` 及队列列表项：
  额外展示每个供应商映射到的**实际模型名**（读 `settingsConfig.env.ANTHROPIC_MODEL`，
  无则回退显示 base_url 的 host）。
- 可选：给「映射到非 Claude 模型」的条目加一个轻量徽标（如 `→ deepseek-chat`），
  让降级链路一目了然。
- 涉及类型：`src/types/proxy.ts` 的 `FailoverQueueItem` 可加可选字段
  `mappedModel?: string` / `upstreamHost?: string`，由后端 DAO 顺带返回，或前端从 provider 详情拼。
  - 若选后端返回：在 `database/dao/failover.rs:get_failover_queue` 的 SELECT 里
    一并取 `settings_config`，解析出模型名填进 `FailoverQueueItem`（Rust 侧）。
  - 若选前端拼（更轻、推荐）：队列项点开时用已缓存的 providers 数据 join 出模型名，**不动后端**。

### M3 — 引导与文档

- 在「添加供应商」或故障转移面板加一段说明/模板：
  「如何把 DeepSeek/GLM/Qwen 接成 Anthropic 兼容备用供应商」——给出需要填的 3 个 env 字段示例。
- `docs/` 写一篇使用指南（中文），含 M1 的验证步骤截图/步骤，作为 e2e 手测脚本。
- 在故障转移面板增加一句提示：备用供应商必须是 **Anthropic 兼容**接口（本路线的前提）。

### M4 — 测试

- 前端：`pnpm test:unit`（vitest + msw）。
  用 msw mock 两个上游：A 返回 5xx/超时、B 返回正常，断言 UI 故障转移状态流转。
- 后端：`cargo test`（在 `src-tauri/`）。
  若 M2 选了「后端返回 mappedModel」方案，给 `get_failover_queue` 补一个断言
  「队列项能解析出 env.ANTHROPIC_MODEL」的单测——断言**关系/不变量**，
  不要写死具体模型名（遵循仓库「禁止 change-detector 测试」规范）。
- 不需要新增协议转换相关测试（本路线无协议桥接）。

---

## 2. 明确不做的事（范围边界）

为避免过度设计，本计划**刻意排除**以下内容（它们属于「方案 B：跨协议」，本次不做）：

- ❌ 不新增 `failover_group_members` 表 / 跨 app 队列。
- ❌ 不改 `ProviderRouter::select_providers` 的核心选择逻辑。
- ❌ 不写 `protocol_bridge.rs`，不做 Anthropic↔OpenAI↔Gemini 协议转换。
- ❌ 不碰 `hot_switch_provider` 的 live 配置写入逻辑。

> 如果将来出现「只有 OpenAI 原生接口、没有 Anthropic 兼容端」的备用模型需求，
> 再单独立项做协议桥接（参考 v1 计划里的 M3–M6）。

---

## 3. 里程碑与验收

| 里程碑 | 内容 | 验收标准 | 状态 |
|--------|------|----------|------|
| M1 | 纯配置验证基线 | A 熔断后自动切 B，Claude Code 无感；A 恢复后自动切回 | ⏳ 待用户提供真实 base_url/key 后实跑 |
| M2 | 队列 UI 显示实际模型名 | 队列每项能看到 `→ <模型名>` | ✅ 已实现（前端 join，未改后端） |
| M3 | 文档 + 添加供应商引导 | 用户照文档能独立配出一条可用降级链 | ✅ 见 `docs/guide-cross-model-failover.md` |
| M4 | 前后端测试通过 | `pnpm test:unit` 绿；新 util 有单测 | ✅ vitest 全量 300 测试通过（含 6 个新 `getModelFromConfig` 测试） |

### 已落地改动清单（M2 + M3 + M4）

- `src/utils/providerConfigUtils.ts`：新增 `getModelFromConfig(settingsConfig, appType)`，
  从供应商配置解析上游实际模型名（Claude 读 `ANTHROPIC_MODEL` 及 SONNET/OPUS/HAIKU 回退，
  Codex/Gemini 读各自字段）。
- `src/components/proxy/FailoverQueueManager.tsx`：通过 `useProvidersQuery` 拉全量供应商，
  前端 join 出 `providerId → model` 映射，在每个队列项下展示 `→ <模型名>` 徽标。
  **纯前端，不改后端。**
- `src/utils/providerConfigUtils.test.ts`：新增 6 个 `getModelFromConfig` 单测
  （含「跨模型映射」核心用例），断言行为而非快照。
- `docs/guide-cross-model-failover.md`：中文使用指南 + M1 验证步骤。

> 注：`tsc --noEmit`（项目配置）零报错；`vitest run` 54 文件 / 300 测试全绿。
> 故障转移的选路/熔断/热切换核心逻辑**未改动**——跨模型能力本就由现有
> `model_mapper.rs` + `select_providers` 提供，本次只补齐了「看得懂、配得对、有文档」。

---

## 4. 待你确认的小问题

1. M2 的模型名展示，倾向**前端 join（不动后端，最轻）** 还是 **后端 DAO 返回**？
   我建议前端 join。
2. 你的 DeepSeek（或其他备用）的 **Anthropic 兼容 base_url 和模型名**是什么？
   有的话我可以在 M1 直接用真实值帮你跑通验证。
3. 是否希望我现在就启动应用、按 M1 把基线跑起来（需要本机能跑 `pnpm tauri dev`，
   Rust 首次编译耗时较长）？还是先只交付 M2/M3 的代码与文档？
