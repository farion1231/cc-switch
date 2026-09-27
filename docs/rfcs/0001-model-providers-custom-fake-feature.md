# RFC 0001: `[model_providers.custom]` 的"虚假功能"复盘

| 字段 | 值 |
| --- | --- |
| 状态 | Accepted（已撤回 + 加护栏） |
| 作者 | cc-switch 维护者 + 2026-09-28 接力会话 |
| 相关 PR | farion1231/cc-switch#7714（含 anti-thundering-herd delay + codex 自建网关工作） |
| 相关 commit | `5973d334` `feat(codex)` + `51788c88` `docs(handoff)` |
| 适用版本 | codex 0.158（更新版 codex 未做回归验证，结论可能反转） |

## TL;DR

在 `[model_providers.custom]` 块下写了 `request_max_retries` / `stream_max_retries` /
`stream_idle_timeout_ms` 三个键，**以为**它们能让 codex 在网关长尾/卡死时自动回收。
实际**不生效**——bogus host + 这三个键后 codex 仍 30s+ 卡死直到外部 timeout。
本文是「为什么误以为生效了」+「如何不复现」的复盘。

## 1. 背景

`cc-switch` 让用户一键切换 codex 的「档位」（每个档 = 一个供应商 = 一份
`~/.codex/config.toml`）。2026-09-28 我们接入两个自建 LLM 网关（`llm.kxpms.cn`
与 `localhost:8782`）作为新的 codex 档，写入

```toml
[model_providers.custom]
name = "kxpms_gateway"
base_url = "https://llm.kxpms.cn/v1"
wire_api = "responses"
requires_openai_auth = true
request_max_retries = 2
stream_max_retries = 2
stream_idle_timeout_ms = 120000
```

意图：在网关返回长尾（claude-sonnet-5 实测 100s 零输出）/ 限流时，让 codex 客户端
2 次重试 + 空闲 120s 后断流，避免无限挂着。

## 2. 误以为生效的链条

| # | 我们做了什么 | 为什么是错的 |
| --- | --- | --- |
| 1 | 看 codex 的 schema / 配置示例 | schema **接受**任意 key 不报错，但「接受」 ≠ 「运行时读取」。codex 的 config parser 对未知 key 是宽容的，没有 schema 校验把关。 |
| 2 | 看 codex 的 source code 注释，把 `[model_providers.*]` 当作"和顶层 config 一样的 namespace" | 实测：这三个键只被**顶层 `config.toml`** 消费，不被 `[model_providers.custom]` 消费。后者是一份独立的 provider descriptor，只描述「这个 provider 怎么连」。 |
| 3 | 跑端到端只验 `claude-opus-5` / `glm-5.2` 等「正常返回」的模型 | 这类路径 < 30s 完成，根本走不到"重试 / 空闲断流"的代码分支。换句话说：**只在快乐路径上做了 e2e**，故障路径没观测到。 |
| 4 | 没写「虚假功能护栏」测试 | 没有任何 test 在失败路径上断言"这两个键生效"。缺少"防回归"的测试资产。 |

## 3. 复现

下面三步可以在本地复现"为什么我们误以为生效了"。

### 3.1 准备 codex 配置（**故意指向 bogus host**）

```toml
# ~/.codex/config.toml
model_provider = "custom"
model = "claude-opus-5"
disable_response_storage = true

[model_providers.custom]
name = "bogus_gateway"
base_url = "http://127.0.0.1:1/v1"
wire_api = "responses"
requires_openai_auth = true
request_max_retries = 2
stream_max_retries = 2
stream_idle_timeout_ms = 120000
```

`127.0.0.1:1` 一定拒绝连接；这是"卡死"路径的代表。

### 3.2 起一个外部超时

```bash
timeout 90 codex exec "hi" --json
```

### 3.3 观察

期望（如果这三个键生效）：~ 60-120s 后 codex 因 stream_idle_timeout_ms 触发断流，
**实际**：codex 仍然卡到 `timeout` 外部命令的 90s 才退出；没有任何"重试""空闲断流"迹象。

**结论**：`[model_providers.custom]` 层下这三个键被 codex 0.158 完全忽略。

### 3.4 旁证：还原成顶层 config 也无效

把同样三个键改写到**顶层** `config.toml`（不进 `[model_providers.custom]`），仍 bogus host
+ `timeout 90`：同样卡满 90s。说明即使放对位置，codex 0.158 也不消费
`stream_idle_timeout_ms` 这个键（`request_max_retries` / `stream_max_retries` 我们
没在顶层继续复测，存疑，但本 RFC 主张**不要写**，等 codex 后续版本明确语义）。

## 4. 修正

### 4.1 从三处删掉

1. `src/config/codexProviderPresets.ts`：两个 preset 的 `[model_providers.custom]` 都清掉这三行，并在同位置留注释指向本 RFC：

   ```toml
   # 重试/空闲限流键：2026-09-28 17:54 实测 bogus host + 这些键后 codex 仍 30s+ 卡死
   # 直到外部 timeout；说明 codex 0.158 在 [model_providers.custom] 层不消费这三键，
   # 这是「虚假功能」。删之，避免给后来人误导。详见 docs/rfcs/0001-...
   ```

2. `~/.cc-switch/cc-switch.db`：现有供应商记录的 `settings_config` 也清掉这三个键
   （手动跑了一次迁移脚本，覆盖 `codex_providers` 表的 JSON）。

3. `~/.codex/config.toml`：live 配置清掉这三个键。配套迁出的是
   `~/.codex/{strict,yolo}.config.toml`（独立 `[profiles.*]`），不在本文讨论。

### 4.2 加护栏测试

`src-tauri/src/codex_config.rs` 新增一个集成测
`self_hosted_gateway_presets_round_trip_through_catalog_pipeline`：把两个预设喂给
`prepare_codex_config_text_with_model_catalog` + 跑 schema 校验，断言**不出现**这三键。
这条测试不依赖网络，只挡"preset 错把键加回来"的回归。

### 4.3 文档

- `docs/handoff/2026-09-28-codex-self-hosted-gateway.md` §5「第二轮审计」写下了
  这条事实结论。
- 本文（RFC 0001）是它的长版本，给后续 contributor 看。

## 5. 教训 / Guardrails（给将来）

| 教训 | 应用场景 |
| --- | --- |
| **"schema 接受 ≠ 运行时消费"**：codex 的 TOML parser 对未知键宽容，是这次误判的根因。 | 任何 codex 配置 PR，都必须 e2e 走「失败路径」并断言「期望行为发生」，不能只看「快乐路径通」。 |
| **"配置表面"与"运行时作用域"是两件事**：`[model_providers.*]` 是 provider descriptor，不是配置 namespace。 | 写 codex 配置前，先看 codex 文档里"where this key lives" 的明确说法；找不到就默认**不写**。 |
| **e2e 必须包含故障分支**：只测 happy path 等于没测。 | cc-switch 任何"档位"接入，必须有"切到 bogus host → 期望行为" 的回归测。 |
| **测试断言要锁「不出现的 key」**：把"不该出现的功能"写在测试里，等于加了守护。 | 本 RFC 的 4.2 节那条集成测就是这种形态。 |
| **删除的键要写注释 + 留文档**：防止后来人"复现"这条 bug。 | preset 里同位置贴了 RFC 链接，handoff 写了复盘。 |

## 6. 后续工作（不在本 RFC 范围内，但相关）

- `cargo test` 加一个 bogus-host 的 e2e job（参考本 RFC §3 的步骤），跑在 GitHub Actions
  上 → 关联 [PR #7714 后续项 #3](#)「CI step」。
- 写一个 sync 脚本（参考 [PR #7714 后续项 #4](#)）从 SSOT 拉模型清单，校验
  `id` 在 `/v1/models` 中存在 → 关联 catalog 数量与 SSOT 同步问题。
- 切档前 dry-run（参考 [PR #7714 后续项 #6](#)），让用户先 diff 再确认切。

## 7. 附录：证据

- `~/.codex/cc-switch-model-catalog.json` 生成前后的 toml diff（保留在
  `~/.cc-switch/backups/gateway-codex-20260928-003325/verification/`）。
- `cargo test --lib codex_config::self_hosted_gateway_presets_round_trip_through_catalog_pipeline`
  输出：跑过 / 失败信息可作为回归样本。
- `git log 5973d334 -p` 看撤回 commit 的 diff（含三处删除 + 注释）。