# issue #7608 修复计划 · Codex × OpenCode-Go × mimo-v2.6-flash → HTTP 400

- Issue：https://github.com/farion1231/cc-switch/issues/7608
- 标题：对于 opencodego 订阅的 mimov2.6flash 模型问题
- 版本 / 环境：CC Switch 3.20.4，Windows，Codex app
- 仓库：`D:/my-project/cc-switch`（origin = yukitakasama/cc-switch，upstream = farion1231/cc-switch，基线分支 `main` @ 793e67d9）
- 编制：2026-10-04 01:20 GMT+8
- **硬性约束：实现完成后禁止提交**（见「C 组 · 提交门禁」）

---

## 0. 问题陈述（原文摘要）

```
CC Switch local proxy failed while handling Codex endpoint /responses.
Provider: OpenCode-Go chat; model: mimo-v2.6-flash;
upstream_status: HTTP 400;
cause: data: {"error":{"param":"","type":"server_error","message":"Streaming response failed: [400] Invalid request parameters"}}
```

用户自述：opencodego 订阅不像 deepseek 那样原生支持 responses 协议。
期望行为：**正确路由 `reasoning_content`**。

---

## 1. 错误链路定位（已核实）

| 环节 | 位置 | 说明 |
|---|---|---|
| ① Codex 发 `/responses` | `src-tauri/src/proxy/server.rs:335-349` | Codex 被写成 `http://127.0.0.1:15721/v1` + `wire_api = "responses"` |
| ② 分支判定 | `src-tauri/src/proxy/handlers.rs:1094` `should_convert_codex_responses_to_chat` | 命中则走 Responses→Chat 转换 |
| ③ 请求构造 | `src-tauri/src/proxy/forwarder.rs:1498-1529` → `transform_codex_chat.rs:272` `responses_to_chat_completions_with_reasoning` | 生成发往上游的 Chat 请求体 |
| ④ 上游 400 | `src-tauri/src/proxy/forwarder.rs:2436-2492` | 非 2xx 一律读 body 变 `ProxyError::UpstreamError`；上游用 `text/event-stream` 包体回 400 → `body_text` 带 `data: ` 前缀，正好解释 issue 里 `cause: data: {...}` |
| ⑤ 错误包装 | `src-tauri/src/proxy/handlers.rs:2140-2279` `build_codex_proxy_error_response` / `codex_proxy_error_json` | 拼出 issue 里那句完整报错（格式已被 `handlers.rs:3824` 单测锁定） |

**关键判定**：字符串 `Streaming response failed` 在全仓（`*.rs` / `*.ts` / `*.tsx` / `*.json` / `*.md`，排除 node_modules）grep **0 命中** → 该文案 100% 来自 OpenCode Zen 网关，是它把我们的请求体下发给模型厂商时、在其内部流式阶段被拒后的包装。

**结论：不是 CC Switch 的 SSE 转换器出错（还没走到），而是「本地发给 zen 的 Chat 请求体里含 zen/下游厂商不接受的参数」。**

---

## 2. 根因（T1 实锤，结论覆盖下方编制时假设）

> 回填时间：2026-10-04 14:00 GMT+8（实施会话）。下方 H1/H2/H3 为编制时假设，保留供追溯。

| 假设 | 结论 | 依据 |
|---|---|---|
| **H1** 占位注入无 provider 门控 | ✅ **成立，即 400 主因** | 见 2.1 |
| **H2** `mimo-v2.6-flash` 不在 OpenCode Go 目录 | ✅ **成立，但属并行缺陷，不是 400 触发点** | 见 2.2 |
| **H3** 透传字段被网关拒收 | ❌ **排除** | 见 2.3 |

### 2.1 H1 实锤

1. **代码路径**：修复前 `backfill_tool_call_reasoning_placeholders`（`transform_codex_chat.rs:1167`）签名只有 `messages: &mut [Value]`，对每条 `role == "assistant" && tool_calls 非空` 的消息无条件调用 `ensure_tool_call_reasoning_content`；后者在拿不到真实 reasoning 时插入 `"reasoning_content": "tool call"`。
2. **无门控通道**：调用链 `responses_to_chat_completions_with_reasoning`(L272) → `append_responses_input_as_chat_messages`(L612) → `backfill_...`(L1167) 全程只传 `messages`，**没有任何 provider 参数**，因此结构上不可能对网关做差异化。
3. **对照面不对称（关键旁证）**：Claude→Chat 的同类注入 `claude.rs:313 should_preserve_reasoning_content_for_openai_chat` 有平台 / vendor 门控（`REASONING_VENDOR_HINTS` + base_url 判定），Codex→Chat 侧完全没有 —— 同一仓库两条路径语义不一致，符合"实现遗漏"而非"设计如此"。
4. **行为对照实验（同一份输入，只换 provider）**：
   - `responses_request_to_chat_injects_placeholder_reasoning_for_bare_tool_call`（直连 Moonshot，`api.moonshot.cn`）→ `messages[0]["reasoning_content"] == "tool call"`；
   - `responses_request_to_chat_skips_placeholder_reasoning_for_opencode_zen`（OpenCode Go，输入结构逐字段相同）→ `messages[0]` 无 `reasoning_content`。
   - 两者唯一差异是 provider → **门控就是分水岭**，同时证明 A1/A2 的断言方向正确。
5. **与 issue 体感吻合**：占位只在「assistant 带 `tool_calls` 且该轮历史没有可用 reasoning」时产生，而 Codex 会话从第二轮起必然携带 tool-call 历史 → 恰好解释"首轮正常、之后必现 400"。

### 2.2 H2 实锤（成立，但不是 400 的触发点）

- 抓取 `https://models.dev/api.json`（2026-10-04）核对 `opencode-go` 目录：`mimo-v2.6-flash` 与 `mimo-v2.6-pro` **均已上架**，而本地 `src/config/codexProviderPresets.ts` 的 OpenCode Go `modelCatalog` 缺这两个 → 与 issue 场景完全对应。
- **后果 1（不触发 400）**：`zen_catalog_effort_levels` 按 `modelCatalog.models[].model` 精确匹配，查不到 → `effort_levels = None` → `"zen"` 分支 `let levels = effort_levels?;` 直接返回 → 完全不发 `reasoning_effort`。所以 H2 **不会**造成 400，只是"思考档位失效"。
- **后果 2（静默改模型）**：`apply_codex_upstream_model` 对不在 catalog 的请求模型覆写为 provider 配置模型（`glm-5.3`）—— 实锤存在，但按 3.3 / T3.3 **不在本次修复范围**。
- 档位声明：`models.dev` 中这两个型号的 `reasoning_options` 为空（网关未暴露 effort 档位）→ 目录条目不声明 `reasoningLevels`（与既有 `mimo-v2.5-pro` 同款），代理查表为 `None` 时不发 `reasoning_effort`，避免发无效档位。
- 输入模态：两个型号 `modalities.input` 含 image → `inputModalities: ["text", "image"]`。

### 2.3 H3 排除理由

- H1 已完整解释 400：同一份输入，去掉占位即不再含任何厂商私有字段，网关不再有拒收对象。
- `stream_options` / `parallel_tool_calls` / `n` 等属**标准 OpenAI 字段**，严格 OpenAI 兼容网关没有拒收理由；且 issue 报错发生在第二轮（携带 tool-call 历史），而 `stream_options` 是每轮都发的 —— 与 H3 预期不符。
- 按 3.3 不顺手重构 `EXTRA_CHAT_PASSTHROUGH_FIELDS`，避免 PR 膨胀。

### 2.4 编制时假设（保留，供追溯）

#### H1（主因，最可能）—— `reasoning_content` 占位注入无 provider 门控

`src-tauri/src/proxy/providers/transform_codex_chat.rs:1167-1179`：

```rust
fn backfill_tool_call_reasoning_placeholders(messages: &mut [Value]) {
    for message in messages.iter_mut() {
        let is_assistant_tool_call = /* role == assistant && tool_calls 非空 */;
        if is_assistant_tool_call {
            ensure_tool_call_reasoning_content(message);   // ← 无任何 provider 门控
        }
    }
}
```

`ensure_tool_call_reasoning_content`（`transform_codex_chat.rs:1182-1200`）在没有任何可用 reasoning 时，无条件插入：

```rust
obj.insert("reasoning_content".to_string(), Value::String("tool call".to_string()));
```

- 该回填在 `append_responses_input_as_chat_messages`（`transform_codex_chat.rs:676`）无条件调用，位于管线末端。
- 注释说明其动机是 kimi/Moonshot、DeepSeek 等 thinking 模型**强制要求**带 `tool_calls` 的 assistant 消息必须带非空 `reasoning_content`。
- **对照面的不对称**：Claude→Chat 方向的同类注入**有门控** ——
  `src-tauri/src/proxy/providers/claude.rs:29` `const REASONING_VENDOR_HINTS = &["deepseek","mimo","xiaomimimo"];`
  `claude.rs:313-337` `should_preserve_reasoning_content_for_openai_chat(provider, body)`
  调用点 `claude.rs:425-429`，测试 `claude.rs:2217 / 2252 / 2289 / 2324`。
- Codex 会话每轮必然产生工具调用 → **第二轮起必现**；首轮（无 tool-call 历史）可能正常，符合"时好时坏"的体感。
- OpenCode Zen 是严格 OpenAI 兼容网关，未知字段 → `Invalid request parameters`。

> 这正是 issue 期望的「正确路由 `reasoning_content`」的具体落点。

#### H2（次因）—— `mimo-v2.6-flash` 不在 OpenCode Go 模型目录

`src/config/codexProviderPresets.ts:3157-3200` 的 OpenCode Go `modelCatalog` 只有 `glm-5.3` / `glm-5.3-flash` / `kimi-k3` / `deepseek-v4-pro` / `deepseek-v4-flash` / `mimo-v2.5-pro`，**没有 `mimo-v2.6-flash`**（该模型只出现在 Xiaomi MiMo 官方预设 `codexProviderPresets.ts:2880`，其 `apiFormat: "openai_responses"` 走原生 Responses）。

两个后果：

1. `zen_catalog_effort_levels`（`src-tauri/src/proxy/providers/codex.rs:599-628`）按 `modelCatalog.models[].model` 精确匹配请求模型，查不到 → `effort_levels = None` → `"zen"` 分支 `let levels = effort_levels?;`（`transform_codex_chat.rs:517-534`）直接返回 `None`，**完全不发 `reasoning_effort`**。不会 400，但思考深度失控。
2. `apply_codex_chat_upstream_model` → `apply_codex_upstream_model`（`codex.rs:488-518`）：请求模型不在 catalog 时被**静默覆写**为 provider 配置模型 `glm-5.3`。用户以为在调 `mimo-v2.6-flash`，实际打的是 `glm-5.3`。

#### H3（待排除）—— 透传字段被拒

`EXTRA_CHAT_PASSTHROUGH_FIELDS`（`transform_codex_chat.rs:29-44`）含 `stream_options`、`parallel_tool_calls`、`n`、`logprobs`、`logit_bias` 等；且 `responses_to_chat_completions_with_reasoning` 末尾会 `inject_openai_stream_include_usage`（`transform_codex_chat.rs:361`）注入 `stream_options`。部分网关在非 stream / 不支持 usage 统计时拒收。

---

## 3. 修复方案

### 3.1 核心（T2，针对 H1）：给占位注入加 provider 门控

`responses_to_chat_completions_with_reasoning`（`transform_codex_chat.rs:272`）**已经**持有 `reasoning_config: Option<&CodexChatReasoningConfig>`，只需一路透传到底即可，无需新增配置通道。

**透传链路（实际实现）**

```
responses_to_chat_completions_with_reasoning (body, reasoning_config, provider)  ← 新增 provider 形参
  └─ append_responses_input_as_chat_messages (input, messages, tool_context, provider)
       └─ backfill_tool_call_reasoning_placeholders (messages, provider)         ← 入口统一短路
            └─ ensure_tool_call_reasoning_content (message)                      ← 保持纯助手，无门控
```

调用点 `forwarder.rs:1518-1522` 传 `Some(provider)`；`responses_to_chat_completions`（单测/旧调用者）传 `None` → 回落"保持既有行为"。

**门控函数（实际实现：采纳「最小变更」方案）**

> ⚠️ 计划编制时建议的「保守默认（未知 provider → 不注入）」在实施时**被否决**，原因见下。

```rust
/// 是否允许给带 tool_calls 的 assistant 消息补 `reasoning_content` 占位。
/// 只在「确实是聚合网关」时关闭；其余场景保持既有行为。
fn should_inject_tool_call_reasoning_placeholder(provider: Option<&Provider>) -> bool {
    !provider.is_some_and(super::codex::is_aggregator_gateway)
}
```

**为什么改成最小变更（否决计划原方案）**

计划 3.1 原意是"仅当非聚合网关 **且** vendor hint 命中才注入"的保守默认。实施时发现反证：

- `claude.rs:26-28` 注释明确写着：Moonshot/Kimi 已于 2026-08 按厂商要求从 `REASONING_VENDOR_HINTS` 移除，并注明「Do not re-add without re-confirming」。计划却要求把 `kimi` / `moonshot` 加回 hints —— **前提不成立**。
- 保守默认会改变**所有直连厂商**的现状行为，且现有 4 个断言"注入占位"的既有单测需全部改造，回归面大。
- 两种方案都能满足 A1–A3 全部验收项，差别只在「未知 provider 的默认行为」。

采纳最小变更的收益：既有占位注入断言单测零改动全绿；不会给直连厂商（GLM 智谱直连 / Qwen / MiniMax / 自定义）引入新的 `reasoning_content is missing` 故障。
已知残留：未消除"其他严格端点也拒收该字段"这一同类隐患，记入 NEXT-STEPS。

**两处与计划的实现差异**

1. 计划建议在 `ensure_tool_call_reasoning_content` 首行加短路。实施改为在 `backfill_tool_call_reasoning_placeholders` **入口统一短路** —— 等价且只判一次，该函数是管线末端唯一入口；`ensure_tool_call_reasoning_content` 保持纯助手语义不变。
2. 计划建议把 `reasoning_config` 一路透传。实施改为**只透传 `provider`** —— 门控条件（是否聚合网关）只取决于 provider 身份，与 `reasoning_config` 无关；而 `reasoning_config` 在聚合网关场景下可能因 `meta.codexChatReasoning` 提前返回而拿不到，反而会误判。

**必须同步检查（已完成）**：Claude→Chat 路径 `claude.rs:313-337` 已加同款聚合网关排除 —— 把原本「模型名命中 vendor hint 就放行」的判定挪到网关判定**之后**，使"走网关的厂商模型"（如 opencode.ai 上的 `mimo-v2.6-flash`）也能被拦下。两条路径语义现对称。

### 3.2 目录 / effort（T3，针对 H2）

- 按 `https://models.dev/api.json`（2026-10-04 抓取）核对 OpenCode Go 实际供应的 mimo 型号 → **补齐 `mimo-v2.6-flash` 与 `mimo-v2.6-pro`** 到 `codexProviderPresets.ts` 的 OpenCode Go `modelCatalog`，`contextWindow: 1048576`，`inputModalities: ["text", "image"]`。
- **不声明 `reasoningLevels`**（与计划原意相反，以实测为准）：models.dev 中这两个型号的 `reasoning_options` 为空，网关未暴露 effort 档位；若强行声明会造成"发出网关不认的档位"。代理查表得 `None` → 完全不发 `reasoning_effort`，与既有 `mimo-v2.5-pro` 同款处理。
- 只补 catalog，不改 `apply_codex_upstream_model` 的静默覆写行为（T3.3 另开 issue）。
- 快照影响：golden 快照 `tests/components/__snapshots__/ProviderForm.presetRows.golden.test.tsx.snap` **无需改动**（该用例不覆盖 OpenCode Go 目录，实测未变化）；仅 `tests/config/codexChatProviderPresets.test.ts` 的两处期望表需要同步。见 B3。

### 3.3 不做的事（防止 PR 膨胀）

- 不改 `codex_proxy_error_json` 的既有文案格式（`handlers.rs:3824` 单测已锁定）。
- 不改 `output_format` 的"声明性、运行时不读"现状（`provider.rs:414-418` 注释）。
- 不新增 UI / i18n 文案（除非 T3 引入用户可见变更）。
- 不顺手重构 `EXTRA_CHAT_PASSTHROUGH_FIELDS`。

---

## 4. 验收标准

### A 组 · 行为验收（功能正确）

| 编号 | 场景 | 断言 |
|---|---|---|
| **A1** | provider = OpenCode Go 预设形态（`apiFormat: "openai_chat"`，`codexChatReasoning.effortValueMode: "zen"`，`outputFormat: "reasoning_content"`，base_url `https://opencode.ai/zen/go/v1`），输入含 `function_call` + `function_call_output` 历史的 Responses 请求 | 转换后 `messages` 中**所有** assistant 消息**不含** `reasoning_content`（上游真实回传的历史 reasoning 除外） |
| **A2** | provider = 直连 kimi / moonshot / deepseek / mimo（name 或 base_url 命中 hints），同样输入 | 带 `tool_calls` 的 assistant 消息**仍含非空** `reasoning_content`（占位 `"tool call"` 或真实值）—— 回归不破坏 |
| **A3** | provider = OpenRouter / SiliconFlow / ModelScope / `opencode.ai` 四类聚合网关 | 均**不注入**占位 `reasoning_content` |
| **A4** | 真机：OpenCode-Go chat provider + `mimo-v2.6-flash`，Codex 连续两轮对话（第二轮必带 tool-call 历史） | `/responses` 返回 200，不再出现 `upstream_status: HTTP 400`；reasoning 内容正确出现在 Responses `output` 的 `reasoning` item（`summary[].text`）中 |
| **A5** | T3 生效后：`mimo-v2.6-flash` 命中 catalog | `zen_catalog_effort_levels` 返回其 `reasoningLevels`，`reasoning_effort` 按档位钳制发出；且请求模型**不再**被覆写成 `glm-5.3` |

> A1–A3、A5 用 `#[cfg(test)] mod tests` 单测判定；A4 为人工真机验收（需要 OpenCode Go API Key）。

### B 组 · 门禁验收（CI 同款，必须全绿）

```bash
cd /d/my-project/cc-switch
pnpm typecheck
pnpm format:check
pnpm test:unit

cd src-tauri
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

| 编号 | 标准 |
|---|---|
| **B1** | 上述 6 条命令全部退出码 0；`cargo test` 无新增 skip / 无新增 ignored |
| **B2** | 新增单测 ≥ 4 个，覆盖 A1 / A2 / A3 各 ≥1 个 + zen effort 目录命中 1 个，全部位于被改文件的 `#[cfg(test)] mod tests` |
| **B3** | 若改动 preset：`tests/config/codexChatProviderPresets.test.ts:108-120 / 371-396` 与 `tests/components/__snapshots__/ProviderForm.presetRows.golden.test.tsx.snap` 必须同步更新，并在交付说明中解释每处快照差异 |
| **B4** | 若新增用户可见文案：四语（`src/i18n/locales/{zh,zh-TW,en,ja}.json`）齐备，`tests/config/localeCoverage.test.ts` 通过 |
| **B5** | 若新增 `CodexChatReasoningConfig` 字段：`src-tauri/src/provider.rs:403-425` + `src/types.ts:157-165` + 受影响预设三处同步，`pnpm typecheck` 覆盖 |
| **B6** | 改动文件数受控：核心修复 ≤ 5 个 Rust 文件；preset 改动仅限 `src/config/codexProviderPresets.ts` + 对应测试/快照 |

### C 组 · 提交门禁（用户硬性要求）

| 编号 | 标准 |
|---|---|
| **C1** | **实现完成后不执行 `git commit` / `git push` / `gh pr create` 中的任何一个**，改动只留在工作区 |
| **C2** | 未经用户明确指令，不得执行 `git add` |
| **C3** | 交付前输出 `git status --short` 与 `git diff --stat` 供人工复核 |
| **C4** | 不在 issue #7608 下发布任何"已修复"性质的评论（未提交 = 未修复） |

---

## 5. Todolist

### T0 · 准备

- [x] **T0.1** 从 `main` 拉本地分支 `fix/issue-7608-codex-chat-reasoning-placeholder`（**不推远端**）
      → ✅ 已建；`git branch -vv` 显示基线 `793e67d9`（= 计划所记 main），**无 upstream 跟踪**，`git ls-remote origin` 无该分支 → 确认未推远端。
- [~] **T0.2** 记录基线：`cargo test`（src-tauri）与 `pnpm test:unit` 的通过数
      → ⚠️ 前端基线已取（`pnpm test:unit`：**7 failed / 1767 passed（1774 tests，156 files）**，失败集中在 `tests/integration/App.test.tsx`、`tests/components/AboutSection.test.tsx`、`tests/components/PiProviderForm.test.tsx`，A/B 回滚到 HEAD 后失败集合完全一致 → 与本改动无关）。
      → ⚠️ Rust 侧**未取到干净整轮基线**：本机存在环境级构建阻塞（§7.1），且 `cargo test` 全量需 `cargo clean` 后单次不中断运行。改用「失败用例归属」法替代：见 §8 与 §7.1。
- [~] **T0.3** 复现 H1（临时打印单测）
      → ⚠️ 未另写临时打印用例。H1 由「静态结构证据（§2.1-1/2/3：链路无 provider 形参 → 结构上不可能差异化）+ 常驻对拍用例（§2.1-4：同一输入只换 provider）」共同实锤，等价且不留临时代码（T4.6 因此无对象可删）。

### T1 · 诊断实锤

- [x] **T1.1** 逐项验证 H1 / H2 / H3 → ✅ 见 §2 / §2.1 / §2.2 / §2.3（H1 ✅ 主因、H2 ✅ 成立但非 400 触发点、H3 ❌ 排除）
- [x] **T1.2** 把实锤结论写回本文件第 2 节（覆盖假设），并据此裁剪 T2/T3 范围 → ✅ §2 已改为结论表 + 证据小节，旧假设降级为 §2.4
- [x] **T1.3** （条件项）实锤指向 H3 时改写 T2 → ✅ N/A：实锤指向 H1，T2 方向不变

### T2 · 核心修复（H1）

- [x] **T2.1** 在 `codex.rs` 抽出聚合网关判定 → ✅ `codex_provider_base_url` / `codex_provider_platform_identity` / `pub(crate) platform_is_aggregator_gateway(name, base_urls)` / `pub(crate) is_aggregator_gateway(provider)`（`codex.rs:761-800`）；`infer_aggregator_platform_config` 签名改为直接收身份串
- [x] **T2.2** 新增 `should_inject_tool_call_reasoning_placeholder(provider)` → ✅ `transform_codex_chat.rs:1169-1177`
- [~] **T2.3** 透传参数 → ✅ 但**只透传 `provider`**，未透传 `reasoning_config`（偏离记录见 §3.1「两处与计划的实现差异」第 2 条）
- [~] **T2.4** 门控短路位置 → ✅ 但由「`ensure_tool_call_reasoning_content` 首行」改为**`backfill_tool_call_reasoning_placeholders` 入口统一短路**（等价、只判一次，`ensure_*` 保持纯助手语义）
- [x] **T2.5** 检查 `claude.rs:313-345` 对称性 → ✅ 已加同款聚合网关排除（判定顺序：网关判定 → 模型名 vendor hint），两条路径语义对称
- [x] **T2.6** 更新 `codex_chat_common.rs` 注释 → ✅ 新增模块级注释，声明本模块为无门控纯助手、门控由上层负责

### T3 · 目录 / effort（H2）

- [x] **T3.1** 核对 models.dev → 补 catalog → ✅ 已抓 `models.dev/api.json` 核对；`mimo-v2.6-flash` / `mimo-v2.6-pro` 已补入 OpenCode Go `modelCatalog`（`contextWindow: 1048576`，`inputModalities: ["text","image"]`）
- [x] **T3.2** 同步测试与快照 → ✅ `tests/config/codexChatProviderPresets.test.ts` 两处期望表已同步（+2 条，`reasoningLevels: null`）；golden 快照**未改动**（用例不覆盖 OpenCode Go，实测 10/10 通过）
- [x] **T3.3** 记录「静默覆写」→ ✅ 已记录于 §2.2 后果 2 与 §3.3；**未开 issue**（按计划留待后续）

### T4 · 测试

- [x] **T4.1** A1 用例 → ✅ `responses_request_to_chat_skips_placeholder_reasoning_for_opencode_zen`（OpenCode Go + tool-call 历史 → 无占位）
- [x] **T4.2** A2 用例 → ✅ `..._keeps_placeholder_reasoning_for_direct_vendors`（直连 Moonshot → 仍有占位）
- [x] **T4.3** A3 用例 → ✅ `..._skips_placeholder_reasoning_for_aggregator_gateways`（覆盖聚合网关集合）
- [x] **T4.4** 改造既有用例 → ✅ `..._injects_placeholder_reasoning_for_bare_tool_call` 已改为传直连 Moonshot provider 形态，原意图保留
- [x] **T4.5** A5 用例 → ✅ `codex::tests::test_apply_codex_upstream_model_keeps_catalogued_mimo_v2_6_flash` + `test_resolve_codex_chat_reasoning_zen_omits_effort_for_mimo_v2_6_flash`
- [x] **T4.6** 删除临时单测 → ✅ N/A（T0.3 未产生临时用例）

### T5 · 门禁与交付

- [x] **T5.1** 跑全 B 组命令，逐条记录退出码 → ✅ 前端 4 条 + Rust 4 条**全部有确定结论**，见 §8「T5.1 · 前端门禁逐条记录」与「T5.1 · Rust 门禁逐条记录」：
      `rustfmt --check`=**0**、`cargo clippy -- -D warnings`=**0**、`cargo test -j 2 --no-fail-fast` **17 个目标全部执行**（15 ok / 2 FAILED，13 个失败全在未改动模块）。
      默认并行下 `cargo test` 因本机提交内存耗尽无法跑通（§7.2），属环境限制而非改动问题。
- [x] **T5.2** 输出 `git status --short` 与 `git diff --stat` → ✅ 见 §8「改动文件清单」与本次交付说明
- [x] **T5.3** A4 真机验收 → ⚠️ **真机未验**：本机无 OpenCode Go API Key，未做端到端 200 验证。已记入 NEXT-STEPS。
- [x] **T5.4** **不提交、不推送、不开 PR**（C1–C4）→ ✅ 全程未执行 `git add` / `git commit` / `git push` / `gh pr create`；改动只留在工作区

### T6 · 归档

- [x] **T6.1** 更新本文件「基线与结果回填」表 → ✅ 见 §8
- [x] **T6.2** 清理临时分支 / 临时文件（保留改动）→ ✅ **不删除分支**（改动尚未提交，分支即工作区载体）；临时文件 `/tmp` 级诊断产物已清理，`src-tauri/Cargo.toml` / `Cargo.lock` 的临时环境绕过已回滚（§7.1）

---

## 6. 关键文件索引

| 文件 | 行 | 作用 |
|---|---|---|
| `src-tauri/src/proxy/providers/transform_codex_chat.rs` | 29-44 | `EXTRA_CHAT_PASSTHROUGH_FIELDS` |
| 同上 | 272-364 | `responses_to_chat_completions_with_reasoning(body, reasoning_config, provider)`（入口，**新增 provider 形参**） |
| 同上 | 295 / 612-678 | `append_responses_input_as_chat_messages`（**新增 provider 形参**） |
| 同上 | 517-534 | `"zen"` effort 钳制（`let levels = effort_levels?;`） |
| 同上 | **1167-1177** | 🆕 `should_inject_tool_call_reasoning_placeholder(Option<&Provider>)` —— **本次修复的门控点** |
| 同上 | **1179-1200** | **H1 主因**：`backfill_tool_call_reasoning_placeholders(messages, provider)` 入口短路 + `ensure_tool_call_reasoning_content`（保持纯助手） |
| 同上 | ~3560-3760 | 新增 A1/A2/A3 用例 + 改造后的 `..._injects_placeholder_reasoning_for_bare_tool_call` |
| `src-tauri/src/proxy/providers/codex.rs` | 28-74 | `codex_provider_uses_chat_completions` |
| 同上 | 488-518 | `apply_codex_chat_upstream_model` / `apply_codex_upstream_model`（静默换模型，本次不改） |
| 同上 | 599-628 | `zen_catalog_effort_levels` |
| 同上 | 761-800 | 🆕 `codex_provider_base_url` / `codex_provider_platform_identity` / `platform_is_aggregator_gateway` / `is_aggregator_gateway` |
| 同上 | 828+ | `infer_aggregator_platform_config(platform: &str)`（签名改为直接收身份串） |
| `src-tauri/src/proxy/providers/claude.rs` | 29 / 313-345 / 425-429 | 对照面：**有门控**的同类注入；T2.5 已加同款聚合网关排除 |
| `src-tauri/src/proxy/providers/codex_chat_common.rs` | 1-9 | T2.6 模块级注释：本模块为**无门控纯助手**，写入前须由上层做平台判定 |
| `src-tauri/src/proxy/forwarder.rs` | 1518-1529 | 请求侧转换调用点（传 `Some(provider)`） |
| 同上 | 2436-2492 | 上游非 2xx → `UpstreamError`（`data: ` 前缀来源） |
| `src-tauri/src/proxy/handlers.rs` | 1094 / 2140-2279 / 3824 | 分支判定 / 错误包装 / 既有格式单测 |
| `src/config/codexProviderPresets.ts` | 3132-3220 | OpenCode Go 预设（**已补 `mimo-v2.6-flash` / `mimo-v2.6-pro`**） |
| 同上 | 2852-2900 | Xiaomi MiMo 预设（`apiFormat: "openai_responses"`，含 mimo-v2.6-flash） |
| `tests/config/codexChatProviderPresets.test.ts` | 108-120 / 371-396 | 已同步两个期望表 |
| `tests/components/__snapshots__/ProviderForm.presetRows.golden.test.tsx.snap` | — | **未改动**（用例不覆盖 OpenCode Go，实测 10/10 通过） |

---

## 7. 可复用命令

```bash
# ── CI 同款全门禁 ────────────────────────────────────────────────
cd /d/my-project/cc-switch && pnpm typecheck && pnpm format:check && pnpm test:unit
cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test

# 打印单测输出（看 body 里有没有 "reasoning_content":"tool call"）
cd /d/my-project/cc-switch/src-tauri && cargo test <test_name> -- --nocapture

# 只跑改动文件的 rustfmt（无需 cargo，绕开 target 锁）
rustfmt --edition 2021 --check src/proxy/providers/transform_codex_chat.rs

# 本机无 rustup shim，cargo/rustc 走固定工具链绝对路径
export PATH="/c/Users/yuki/.rustup/toolchains/1.95-x86_64-pc-windows-msvc/bin:$PATH"
```

### 7.1 ⚠️ 本机环境坑：`indexmap 1.9.3` 构建脚本探针失败（`schemars` E0107）

**症状**：`cargo test` / `cargo check` 在编译 `schemars 0.8.22` 时报

```
error[E0107]: struct takes 3 generic arguments but 2 generic arguments were supplied
  --> .../schemars-0.8.22/src/lib.rs:12:32
12 | pub type Map<K, V> = indexmap::IndexMap<K, V>;
note: struct defined here, with 3 generic parameters: `K`, `V`, `S`  (indexmap-1.9.3/src/map.rs:76)
```

**根因链（本次新查明，比既往"RUSTFLAGS 规避法"更精确）**：

1. `indexmap 1.9.3` 的 `Cargo.toml` **没有 `default = ["std"]`**，其 `IndexMap<K, V, S = RandomState>` 的默认 `S` 只在 `#[cfg(has_std)]` 下存在。
2. `has_std` 由 `indexmap/build.rs` 决定：若 `CARGO_FEATURE_STD` 未设置，则用 `autocfg::new().emit_sysroot_crate("std")` **探测**；探测失败 → 只发 `rustc-check-cfg` 不发 `rustc-cfg=has_std`，可在 `target/debug/build/indexmap-*/stderr` 看到 `warning: autocfg could not probe for \`std\``。
3. 探测失败的真因（用转发 shim 抓到）：autocfg 的探测通过 **stdin 管道**把源码喂给 rustc，而本机 Rust 标准库创建匿名管道时失败：

```
T1 output() OK                       # 只管道 stdout/stderr -> 正常
T2 spawn ERR: Os { code: 231, kind: Uncategorized,
                   message: "所有的管道范例都在使用中。" }   # 管道 stdin -> ERROR_PIPE_BUSY
```

→ **本机无法创建"管道 stdin"**，凡是用 `Stdio::piped()` 当 stdin 的构建脚本都会失败。`esbuild` 的 postinstall、`pnpm install` 的 `EBUSY` 大概率同源。
4. 于是 `indexmap` 丢失 `has_std` → `IndexMap<K,V>` 只有 2 个泛型实参 → `schemars` 用 2 参形式 → E0107。

**规避法 A（既往会话惯用，无文件改动，但会让全树失效重编）**：

```bash
RUSTFLAGS="--cfg has_std" cargo test
```

**规避法 B（本次采用，重编面更窄）**：给 `indexmap 1.x` 显式加上 `std` feature，使其构建脚本走
`CARGO_FEATURE_STD` 分支、完全跳过探测。注意 **edition 2021 → resolver v2 把 build-dependency 的 feature 单独解析**，
`schemars` 是 `tauri-build` 的 build-dep，所以 **`[build-dependencies]` 与 `[dependencies]` 两处都要加**（并因 `indexmap` 名字已被 2.x 占用而需要别名）：

```toml
[build-dependencies]
indexmap1 = { package = "indexmap", version = "1.9", features = ["std"] }

[dependencies]
indexmap1 = { package = "indexmap", version = "1.9", features = ["std"] }   # 同款
```

> ⚠️ 该改动**纯属本机环境规避，与 issue #7608 无关，交付前必须还原**（本次已在交付前 `git checkout -- src-tauri/Cargo.toml src-tauri/Cargo.lock` 还原）。

**规避法 C（验证探针本身）**：

```bash
export PATH="/c/Users/yuki/.rustup/toolchains/1.95-x86_64-pc-windows-msvc/bin:$PATH"
cd src-tauri
OUT_DIR='D:\my-project\cc-switch\src-tauri\target\debug\build\indexmap-<hash>\out' \
TARGET=x86_64-pc-windows-msvc CARGO_FEATURE_STD=1 \
  ./target/debug/build/indexmap-<hash>/build-script-build.exe
# 期望输出：cargo:rustc-cfg=has_std   （不再出现 probe warning）
```

### 7.2 ⚠️ 本机环境坑（二）：页面文件耗尽导致 `cargo test` 产物半截（`os error 1455`）

> **重要更正**：本节结论取代此前"`cargo check` 与 `cargo test` 混跑导致 `.rmeta`/`.rlib` 不一致"的猜测 —— 那个归因**是错的**，`cargo clean` 后照样复现即证伪。

**症状**（`cargo clean` 后单次不中断跑全量 `cargo test` 仍然失败）：

```
error[E0786]: found invalid metadata files for crate `cc_switch_lib`
   = note: failed to mmap file '...\target\debug\deps\libcc_switch_lib.rlib':
           页面文件太小，无法完成操作。 (os error 1455)
error[E0786]: found invalid metadata files for crate `pxfm` which `cc_switch_lib` depends on
   = note: failed to mmap rmeta metadata: '...\libpxfm-45b82c53c371d5ca.rmeta'
   = note: failed to mmap file '...\libpxfm-45b82c53c371d5ca.rlib': 页面文件太小，无法完成操作。 (os error 1455)
error: only metadata stub found for `rlib` dependency `core` please provide path to the corresponding .rmeta file
error[E0463]: can't find crate for `cc_switch_lib`      ← bin 与 20+ 集成测试目标全部因此倒下
```

**根因**：`os error 1455` = **`ERROR_COMMITMENT_LIMIT`**，即**提交内存（页面文件）不足**。
并行编译时多个 `rustc` + linker 同时申请提交内存，超限后 `mmap` 失败 → `rlib`/`rmeta` 只写了一半 →
下游 rustc 判定"元数据无效 / 只是 stub"。

**一次解释掉四个此前互相独立的"怪现象"**：

| 现象 | 正确解释 |
|---|---|
| `E0460 found possibly newer version of crate` | 不是"版本更新"，是**上一次读取时 mmap 失败**的残留产物 |
| `crate X required to be available in rlib format, but was not found in this form` | `rlib` 被写坏，只剩 `rmeta` 可用 |
| `cargo test --lib` 能过、全量 `cargo test` 必挂 | lib 目标并行 linker 少；全量含 bin + 20 多个集成测试，内存峰值高得多 |
| `cargo clean` 也救不了 | **不是缓存脏，是内存不够** |

**排除项（已实测）**：磁盘充足（D: 98 GB / C: 23 GB 可用）；sysroot 完好（`libcore` rlib 2.3 MB，非 stub）；无并发构建进程。

**规避法**：降低并行度 —— `cargo test -j 2`（或 `-j 1`）。根治需调大 Windows 页面文件 / 关闭其他吃内存的程序。

| 命令 | 说明 |
|---|---|
| `cargo test -j 2` | 首选：编译与链接的并发峰值压到提交内存以内 |
| `cargo test -j 1` | 兜底：再不行就串行，代价是慢 |
| 调大页面文件 | 根治；系统属性 → 高级 → 性能 → 虚拟内存 |

**与 7.1 的区别**：7.1 是**构建脚本 spawn 匿名管道失败**（`ERROR_PIPE_BUSY`，code 231，影响的是 `indexmap` 的 autocfg 探测）；7.2 是**提交内存耗尽**（`ERROR_COMMITMENT_LIMIT`，code 1455，影响的是所有大目标的 rpm/链接）。两者都属本机环境，与 #7608 无关。

---

### 7.3 其它本机约束

- `extract_repo_archive_accepts_real_world_sized_skill_repos`（`services::skill::tests`）会写 **13,248 个文件**，本机实测 704 s CPU 后仍未结束 → 属**环境性病态慢**，跑门禁时用 `--skip` 排除；**不是代码问题，也不在代码里 skip**。
- `pnpm install` 的 `esbuild` postinstall 在本机 `EBUSY` 失败 → 前端门禁改用 `node node_modules/.../<bin>` 直接调二进制。
- `codex_config::tests::resolve_catalog_rejects_symlink_escaping_config_dir` 需要 Windows **开发者模式**（`symlink_dir`）。
- `services::model_pricing::*` / `services::skill::*` 多个用例共用全局 `TestHomeGuard`，并行跑会互相串扰 → 既有失败，非本次引入。
  规避：降并行度（`cargo test -j 2`）或对失败用例加 `--test-threads=1` 复跑。另有 2 个用例属**残留态 flaky**（marker / 真实 opencode 配置内容），随运行次序浮动。

---

## 8. 基线与结果回填

| 项 | 执行前（基线） | 执行后 |
|---|---|---|
| `cargo test --lib` 通过数 | ⚠️ **未取到干净整轮基线**（环境阻塞，见 §7.1 / §7.2；且这批用例**有状态**，做不到独立 A/B） | **默认并行：3113 passed / 20 failed**；**`-j 2`：3124 passed / 9 failed**（推荐读数）。默认并行下的 20 个失败中，11 个已被「串行复跑 + `-j 2` 复跑」双重证明为并行串扰 / 残留态 flaky，**余 9 个为稳定环境性失败**，全部位于未改动模块（`codex_config` / `model_pricing` / `skill`）。**本次新增 / 改造的 7 个用例全部 ok** |
| `cargo test`（全量） | 同上 | **默认并行：❌ 未跑成** —— 编译阶段 `os error 1455`（提交内存耗尽），bin + 20 个集成测试目标未产出，0 个 test target 运行（见 §7.2）。<br>**`-j 2 --no-fail-fast`：✅ 编译全通、17 个目标全部执行 —— 15 ok / 2 FAILED**（`lib` 3122 passed/11 failed、`tests/skill_sync.rs` 5 passed/2 failed），**13 个失败全部在 `codex_config` / `model_pricing` / `skill`，与改动集合零交集**；`tests/golden`（含 `codex_red_lines.rs`）**39/39 ok**，`tests/proxy_commands` **ok** |
| `pnpm test:unit` 通过数 | **1767 passed / 1774**（156 files，7 failed / 3 files） | **1785 passed / 1794**（158 files，9 failed / 3 files）；失败文件集合与基线**完全相同** |
| 实际根因（覆盖第 2 节假设） | — | **H1 成立**（占位注入无 provider 门控 → 聚合网关拒收 `reasoning_content`）；H2 成立但非 400 触发点；H3 排除。详见 §2 |
| 改动文件清单 | — | 7 个文件（5 Rust + 1 preset + 1 测试），**+419 / −49**；另有 1 个新增未跟踪文档（本计划文件）。见下方「T5.2 · 改动文件清单」 |

### T5.1 · 前端门禁逐条记录

| 命令 | 退出码 | 结论 |
|---|---|---|
| `pnpm format:check` | **0** | `All matched files use Prettier code style!` |
| `pnpm typecheck`（`tsc --noEmit`） | **0** | 无类型错误（`CodexChatReasoningConfig` 字段未新增，B5 不适用） |
| `pnpm test:unit` | 见下 | **3 failed files / 9 failed tests**，全部为**既有环境性失败**，与本改动无关 |

**`pnpm test:unit` 既有失败说明（A/B 证明）**：把 `src/config/codexProviderPresets.ts` 与 `tests/config/codexChatProviderPresets.test.ts` 一并 `git checkout` 回 HEAD 后重跑，失败文件集合**完全相同**（`tests/integration/App.test.tsx`、`tests/components/AboutSection.test.tsx`、`tests/components/PiProviderForm.test.tsx`）。把 `--testTimeout` 提到 30000ms 后仅剩 `App.test.tsx` 的 3 个用例失败 → 属**加载超时抖动**（本机 setup/collect 阶段耗时 500–1000s），非逻辑回归。
> 附带观察，佐证"抖动"而非"回归"：基线那次只收集到 **156** 个文件，当前树是 **158** 个，而被回滚的两个文件都是**已存在**文件（回滚不会减少文件数）→ 差的 2 个文件就是基线那次**因加载超时未被收集**（也因此基线只报 7 个失败、当前报 9 个）。

**与本改动直接相关的用例全绿**：`tests/config/codexChatProviderPresets.test.ts` **9/9**；`tests/components/ProviderForm.presetRows.golden.test.tsx` **10/10**（golden 快照未改动）；`tests/config/localeCoverage.test.ts` **9/9**（B4 不适用，未新增文案）。

### T5.1 · Rust 门禁逐条记录

| 命令 | 退出码 | 结论 |
|---|---|---|
| `cargo fmt --check`（等价：`rustfmt --edition 2021 --check` 逐文件） | **0** | ✅ 5 个改动文件全部通过；另确认 5 个文件内**无任何 `println!` / `dbg!` / `eprintln!` 残留** |
| `cargo clippy -- -D warnings` | **0** | ✅ 通过；**0 warning / 0 error**；`cc-switch v3.20.4` 本体已被实际检查（`Compiling cc-switch` + `Finished dev profile in 2m 45s`），且全程**未再出现 `schemars` E0107**，反向确认 §7.1 的绕过生效 |
| `cargo test`（全量，默认并行，`--skip extract_repo_archive_...`） | **101** | ❌ **环境性失败**：编译阶段即挂（`os error 1455`），**bin + 20 个集成测试目标全部未编译出来，0 个 test target 实际运行**。见 §7.2 |
| `cargo test --lib`（默认并行，`--skip`） | **101** | **3113 passed / 20 failed / 10 ignored / 1 filtered out**，耗时 384.64 s。⚠️ 20 个失败**全部位于未改动模块**，其中 9 个是并行串扰（下一条证明），11 个为环境性确定失败 |
| `cargo test --lib -- --test-threads=1 <20 个失败用例>` | **101** | **9 passed / 11 failed** → 串行后 9 个转为 ok，**证明这 9 个是并行串扰**，余 11 个为确定失败 |
| `cargo test -j 2 --lib`（**降并行度**，`--skip`） | **101** | **3124 passed / 9 failed / 10 ignored / 1 filtered out**，耗时 323.00 s。**失败从 20 → 9**：消失的 11 个即并行串扰项，剩余 9 个与本改动零交集。§7.2 的 `-j 2` 规避法被这一步直接验证有效 |
| `cargo test -j 2 --no-fail-fast`（**全 17 个目标**，`--skip`） | **101** | ✅ **编译全通、17 个目标全部实际执行**（这是默认并行下唯一跑不成的场景，§7.2 的规避法彻底验证）。**15 个目标 ok / 2 个 FAILED**，失败用例 13 个（lib 11 + `skill_sync` 2），**全部在 `codex_config` / `services::model_pricing` / `services::skill`**，与改动集合零交集。逐目标结果见下方 |

**`cargo test -j 2 --no-fail-fast` 逐目标结果（17 个目标）**

| 目标 | 结果 |
|---|---|
| `unittests src/lib.rs`（lib） | ❌ **FAILED — 3122 passed / 11 failed / 10 ignored / 1 filtered**（389.83 s） |
| `unittests src/main.rs` | ✅ ok — 0 passed |
| `tests/app_config_load.rs` | ✅ ok — 4 |
| `tests/app_type_parse.rs` | ✅ ok — 2 |
| `tests/deeplink_import.rs` | ✅ ok — 2 |
| **`tests/golden/main.rs`**（含 `codex_red_lines.rs`，覆盖本次重构的 `codex.rs` 路由） | ✅ **ok — 39 passed**（114.00 s） |
| `tests/hermes_roundtrip.rs` | ✅ ok — 2 |
| `tests/import_export_sync.rs` | ✅ ok — 19 |
| `tests/mcode_commands.rs` | ✅ ok — 3 |
| `tests/mcp_commands.rs` | ✅ ok — 28 |
| `tests/profile_roundtrip.rs` | ✅ ok — 7 |
| `tests/prompt_live_sync.rs` | ✅ ok — 2 |
| `tests/provider_commands.rs` | ✅ ok — 11 |
| `tests/provider_service.rs` | ✅ ok — 50（327.61 s） |
| **`tests/proxy_commands.rs`** | ✅ **ok — 1** |
| `tests/skill_sync.rs` | ❌ FAILED — 5 passed / **2 failed** |
| `tests/support.rs` | ✅ ok — 0 |

**`-j 2` 全量下的 13 个失败用例（逐条归因，与改动零交集）**

```
# lib 目标（11）
codex_config::tests::resolve_catalog_rejects_symlink_escaping_config_dir        ← 环境：symlink_dir 需 Windows 开发者模式
codex_config::tests::missing_or_malformed_marker_never_establishes_ownership    ← 残留态 flaky（marker）
services::model_pricing::tests::batch_update_and_delete_are_persisted_to_local_file      ← 环境：本地定价文件计数 0
services::model_pricing::tests::creates_local_file_with_auto_sync_disabled_by_default    ← 同上
services::model_pricing::tests::empty_override_file_does_not_roll_back_builtin_pricing_repairs  ← 同上
services::model_pricing::tests::reloads_manual_file_edits_and_deletion_tombstones        ← 同上
services::skill::tests::mcode_import_checks_native_copies_and_deploys_selected_skills    ← 环境：残留 test-skill / NotFound
services::skill::tests::mcode_skill_update_rolls_back_deployments_when_database_is_read_only  ← 同上
services::skill::tests::migrate_storage_safely_leaves_an_existing_pi_ssot_alias          ← 同上
services::skill::tests::pi_skill_state_follows_native_directory_presence                 ← 同上
services::skill::tests::uninstall_with_a_missing_source_does_not_backup_or_delete_the_pi_directory  ← 同上（残留态 flaky，时现时隐）

# tests/skill_sync.rs 集成目标（2）
sync_to_app_removes_disabled_and_orphaned_ssot_symlinks
restore_skill_backup_restores_files_to_ssot_and_current_app
```

> **为什么 9 → 11**：`missing_or_malformed_marker...`、`opencode_config::selection_and_crud...`、`skill::uninstall_with_a_missing_source...` 三个是**残留态 flaky**，随运行次序 / 残留文件状态在「过/不过」之间浮动；`-j 2` 两次跑分别命中 9 与 11。**稳定必失败的是 9 个**。

**✅ 本次新增 / 改造的 7 个用例全部 ok**（`cargo test --lib` 实测）：

```
test proxy::providers::codex::tests::test_apply_codex_upstream_model_keeps_catalogued_mimo_v2_6_flash ... ok
test proxy::providers::codex::tests::test_resolve_codex_chat_reasoning_zen_omits_effort_for_mimo_v2_6_flash ... ok
test proxy::providers::transform_codex_chat::tests::responses_request_to_chat_injects_placeholder_reasoning_for_bare_tool_call ... ok
test proxy::providers::transform_codex_chat::tests::responses_request_to_chat_keeps_placeholder_reasoning_for_direct_vendors ... ok
test proxy::providers::transform_codex_chat::tests::responses_request_to_chat_preserves_real_reasoning_on_aggregator_gateway ... ok
test proxy::providers::transform_codex_chat::tests::responses_request_to_chat_skips_placeholder_reasoning_for_aggregator_gateways ... ok
test proxy::providers::transform_codex_chat::tests::responses_request_to_chat_skips_placeholder_reasoning_for_opencode_zen ... ok
```

**20 个失败的串行 / 并行对比矩阵（排他性核心证据）**

> **`-j 2` 复跑后的收敛**：`cargo test -j 2 --lib` 只剩 **9 个失败**，即下表中「串行=FAIL」集合**去掉** `missing_or_malformed_marker_never_establishes_ownership` 与 `opencode_config::selection_and_crud_preserve_selected_file_and_leave_other_file_untouched` 两项 —— 说明这 2 项属**残留态 flaky**（marker / 真实 opencode 配置内容），随运行次序浮动；**真正稳定的环境性失败是 9 个**。

| 用例 | 并行 | 串行 | 判定 |
|---|---|---|---|
| `mode::controller::mode_tests::codex_a_login_that_changes_before_the_write_stops_it` | FAIL | **ok** | 并行串扰 |
| `mode::controller::mode_tests::codex_editor_saves_key_fields_to_the_row_and_global_edits_to_live` | FAIL | **ok** | 并行串扰 |
| `mode::controller::mode_tests::codex_keyring_logins_keep_requires_openai_auth_on_the_preservation_setting` | FAIL | **ok** | 并行串扰 |
| `mode::controller::mode_tests::codex_routes_between_official_and_third_party_contracts` | FAIL | **ok** | 并行串扰 |
| `mode::controller::mode_tests::entering_and_leaving_proxy_mode_only_touches_key_and_exclusive_fields` | FAIL | **ok** | 并行串扰 |
| `mode::controller::mode_tests::grok_proxy_writes_the_route_table_through_the_engine` | FAIL | **ok** | 并行串扰 |
| `mode::controller::mode_tests::stack_models_join_the_claude_contract_and_leave_with_it` | FAIL | **ok** | 并行串扰 |
| `services::provider::codex_client_catalog::tests::records_only_changes_and_trims` | FAIL | **ok** | 并行串扰（期望 `since_ms:18` 得到 `17`，时间精度 + 共享全局态） |
| `services::provider::tests::deleted_codex_account_recovers_after_persisted_startup` | FAIL | **ok** | 并行串扰 |
| `codex_config::tests::resolve_catalog_rejects_symlink_escaping_config_dir` | FAIL | FAIL | **环境**：`symlink_dir` 需 Windows 开发者模式，符号链接未被拒 |
| `codex_config::tests::missing_or_malformed_marker_never_establishes_ownership` | FAIL | FAIL | 环境：marker 文件被别处残留（`-j 2` 下**通过**，属残留态 flaky） |
| `opencode_config::tests::selection_and_crud_preserve_selected_file_and_leave_other_file_untouched` | FAIL | FAIL | 环境：真实 opencode 配置内容与期望不符（`-j 2` 下**通过**，属残留态 flaky） |
| `services::model_pricing::tests::batch_update_and_delete_are_persisted_to_local_file` | FAIL | FAIL | 环境：本地定价文件计数为 0（期望 1） |
| `services::model_pricing::tests::creates_local_file_with_auto_sync_disabled_by_default` | FAIL | FAIL | 同上 |
| `services::model_pricing::tests::empty_override_file_does_not_roll_back_builtin_pricing_repairs` | FAIL | FAIL | 同上 |
| `services::model_pricing::tests::reloads_manual_file_edits_and_deletion_tombstones` | FAIL | FAIL | 同上 |
| `services::skill::tests::mcode_import_checks_native_copies_and_deploys_selected_skills` | FAIL | FAIL | 环境：`NotFound`（系统找不到指定的文件） |
| `services::skill::tests::mcode_skill_update_rolls_back_deployments_when_database_is_read_only` | FAIL | FAIL | 环境：残留 `test-skill`（"已存在同名但内容不同的 Skill"） |
| `services::skill::tests::migrate_storage_safely_leaves_an_existing_pi_ssot_alias` | FAIL | FAIL | 同上 |
| `services::skill::tests::pi_skill_state_follows_native_directory_presence` | FAIL | FAIL | 同上 |

**为什么可以判定「非本次改动引入」**

1. **模块零交集**：**9 个稳定环境性失败**全部落在 `codex_config` / `services::model_pricing` / `services::skill`；本次改动只碰 `proxy::providers::*` + `proxy::forwarder.rs`。
2. **断言内容不经过改动路径**：它们断言的是「本地定价文件的条数」「Pi skill 目录的存在性」「opencode 配置文件的文本」「catalog 路径的符号链接规范化」——都不触发 Responses→Chat 转换或聚合网关判定。
3. **11 个并行失败已被「串行重跑 + `-j 2` 复跑」双重交叉证明是共享全局态（`TestHomeGuard` / 真实用户目录）串扰**：串行下 9 个转 ok；`-j 2` 下另有 2 个残留态 flaky 也转 ok。与代码逻辑无关。
4. **集成测试完全不覆盖本次改动**：`grep -rlE "responses_to_chat_completions|reasoning_content|codex_chat|should_convert_codex_responses_to_chat" src-tauri/tests/` **0 命中**；`tests/golden/codex_red_lines.rs` 只测路由表 / 密钥注入，用的是 `relay.example` / `bridge.example` 等非聚合网关地址。

**⚠️ 诚实声明（未做与未验的部分）**

- **未做「改动前 vs 改动后」的干净 A/B**：这批失败是**有状态**的（会残留 marker / `test-skill` / 定价文件），连续两次运行不独立，A/B 结论会被状态污染，故改用「串行 vs 并行矩阵 + 模块零交集 + 集成测试零覆盖」三重排除法。
- **未在干净机器 / CI 上验证全套**：本机提交内存与测试卫生度都不足以给出权威全绿结论。**权威门禁应为上游 CI**。

### T5.2 · 改动文件清单

**交付前实测 `git diff --stat`（已回滚临时环境绕过）**：

```text
 src-tauri/src/proxy/forwarder.rs                        |   1 +
 src-tauri/src/proxy/providers/claude.rs                 |  26 ++-
 src-tauri/src/proxy/providers/codex.rs                  | 155 +++++++++++--
 src-tauri/src/proxy/providers/codex_chat_common.rs      |  12 +
 src-tauri/src/proxy/providers/transform_codex_chat.rs   | 250 +++++++++++++++++++--
 src/config/codexProviderPresets.ts                      |  17 ++
 tests/config/codexChatProviderPresets.test.ts           |   7 +
 7 files changed, 419 insertions(+), 49 deletions(-)
```

**交付前实测 `git status --short`**：

```text
 M src-tauri/src/proxy/forwarder.rs
 M src-tauri/src/proxy/providers/claude.rs
 M src-tauri/src/proxy/providers/codex.rs
 M src-tauri/src/proxy/providers/codex_chat_common.rs
 M src-tauri/src/proxy/providers/transform_codex_chat.rs
 M src/config/codexProviderPresets.ts
 M tests/config/codexChatProviderPresets.test.ts
?? .workbuddy/                      ← 本地工作区元数据（仅剩 memory/ 记忆日志），不属本次交付、未暂存
?? docs/issue-7608-fix-plan.md      ← 本计划文档（新增，未跟踪）
```

**分支与提交状态（满足 C1–C4）**：分支 `fix/issue-7608-codex-chat-reasoning-placeholder`，**无 upstream**（`fatal: no upstream configured`，即未推送）；`git log -1` 仍为 `793e67d9`，**HEAD 未移动 → 全程无 commit / 无 push / 无 PR / 无 `git add`**。

核心修复落在 **5 个 Rust 文件**（`transform_codex_chat.rs` / `codex.rs` / `claude.rs` / `forwarder.rs` / `codex_chat_common.rs`）→ 满足 **B6**；preset 改动仅 1 个源文件 + 1 个测试文件，快照零改动。

> ✅ 已确认：`src-tauri/Cargo.toml` 与 `src-tauri/Cargo.lock` 的**临时环境绕过**（§7.1，含早前误试留下的 `autocfg 1.5.0→1.5.1`）已在交付前 `git checkout --` 回滚，**不在最终 diff 中**；`.workbuddy/` 下的诊断残留（shim / diag / 各类日志 / models.dev 快照 / `bak/` / `diagout/`）已清理，关键日志已归档到 `C:/Users/yuki/docs/2026-10-04/1350_开发_cc-switch-issue7608修复实现/evidence/`。

---

## 2026-10-05 追加：修复 PR #7855 评审 P2（拆分占位注入与真实推理回放）

### 评审意见（issuecomment-5987046297，GPT-6 Astra，P2）

原 `claude::should_preserve_reasoning_content_for_openai_chat` 在命中聚合网关名单时**整体**
返回 `false`，导致整个 `preserve_reasoning_content` 被关掉。这一个bool 同时控制两件事：

1. 补虚构占位（`"tool call"` / `"[redacted thinking]"`）——**确实该关**（网关 400）；
2. 回放 assistant 历史里**真实存在**的 thinking——**不该关**。

后果：走网关的厂商模型（如 opencode.ai 上的 `mimo-v2.6-flash`）在工具续轮里，工具调用照发，
配套的推理上下文却被静默丢弃，破坏跨轮推理往返；且与 Codex 路径（只门控末端占位回填，
真实 reasoning 始终经 `attach_reasoning_content_field` 附挂）行为不一致。

### 修复方案

新增 `transform::ReasoningContentPolicy`，把两个决策拆成独立字段：

| 场景 | `replay_real_reasoning` | `inject_placeholder` |
|---|---|---|
| `VENDOR`（直连 DeepSeek / MiMo） | true | true |
| `NONE`（通用 OpenAI-compatible） | false | false |
| `GATEWAY`（聚合 / 托管网关） | **true** | false |

- `transform.rs`：`anthropic_to_openai_with_reasoning_content` 与 `convert_message_to_openai`
  改收策略而非 bool；写出点改为「有真实 thinking → 看 `replay_real_reasoning`；
  无 → 看 `inject_placeholder`」。`redacted_thinking` 归入占位（密文不可恢复）。
  另实现 `From<bool>` 保持旧语义映射（`true`→VENDOR、`false`→NONE）。
- `claude.rs`：`should_preserve_reasoning_content_for_openai_chat` →
  `reasoning_content_policy_for_openai_chat`，网关分支返回 `GATEWAY`（而非等价于 `NONE`）。
- `codex_chat_common.rs`：同步模块级注释，指向新函数名并强调两个决策相互独立。

### 测试

新增 9 项（`transform.rs` 5 + `claude.rs` 4），全部经完整Claude 调用链
`transform_claude_request_for_api_format`：

- 网关 + 真实 thinking → 断言**保留**（4 个网关：OpenCode Zen / SiliconFlow / ModelScope / OpenRouter）
- 网关 + 裸 `tool_use` → 断言**不补**占位
- 网关 + `redacted_thinking` → 断言**不补**占位
- 网关优先于 model hint（`deepseek-*` 模型走网关 → 走GATEWAY 而非 VENDOR）
- 直连厂商对照组（`VENDOR` 回放 + 补占位）、通用路径（`NONE` 全不发）、`From<bool>` 兼容映射

**反向验证**：把`GATEWAY.replay_real_reasoning` 临时改回 `false` 复现旧行为后，
2 项测试立即 FAILED（`left: Null` vs `right: "I should call the tool."`），
证明测试真实覆盖该缺陷；随后已还原。

### 验证结果

```text
cargo fmt -- --check                → clean
cargo clippy --lib                  → 0 error / 0 warning
cargo test --lib -j 2 --no-fail-fast -- proxy::
  → 1490 passed; 0 failed（连跑 3 次一致）
```

> 注：`proxy::forwarder::tests::codex_stack::stacked_requests_drop_hosted_web_search_only_where_it_is_rejected`
> 曾单次 FAILED，单独重跑通过、连跑 3 次全绿，且该测试不涉及 `reasoning_content`
> —— 属本机并行执行的环境性 flake，非本次回归。
