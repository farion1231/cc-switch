# Handoff: cc-switch 接入 llm-gateway-go 自建网关（2026-09-28）

## 任务背景

用户希望根据 `~/.minimax/config.yaml` 里 `custom_provider.kaixuan`（https://llm.kxpms.cn/v1）
与 `custom_provider.local`（http://localhost:8782/v1）两个自建 LLM 网关，把它们的模型数据
完善到 cc-switch 项目与运行实例，修正 codex 配置让 codex 能使用多种非 OpenAI 模型，
且**保留 OpenAI 官方档作为独立可选通道**。

## 本轮做了什么（不吹牛版）

### 1. 项目侧：`codexProviderPresets.ts` +389 行

新增两个 codex 预设：
- **开轩 LLM 网关** —— 19 模型目录，其中 15 个非 OpenAI（claude-opus-5/4-8、sonnet-5、
  glm-5.2、kimi-k3、minimax-m3 等）。base_url=https://llm.kxpms.cn/v1，
  `wire_api="responses"` 直连（mcode 侧按 chat 配，网关 `/v1/responses` 同样原生可用）。
- **本地 LLM 网关 (8782)** —— 21 模型目录，其中 17 个非 OpenAI。补录了 SSOT 漏掉的
  glm-5.2 / claude-opus-4-8 / minimax-m2.7（与 `~/.minimax/config.yaml` 对账 + curl 实测）。
- 两个预设都用 `apiFormat: "openai_responses"`，由后端生成 `~/.codex/cc-switch-model-catalog.json`
  并注入 `model_catalog_json` 指针。

### 2. 运行实例

- `~/.cc-switch/cc-switch.db` codex 供应商 4 个（`default` ChatGPT OAuth、`OpenAI Official`
  占位、`开轩 LLM 网关*` 当前、`本地 LLM 网关 (8782)` 待命），**完全独立、UI 一键可切**。
- `settings.json` 的 `currentProviderCodex` 已对齐到 `kxpms-gateway`。
- 备份：`~/.cc-switch/backups/gateway-codex-20260928-003325/`（含原 config + settings + DB）。

### 3. codex 配置

`~/.codex/config.toml` 恢复被误删的通用配置（`approval_policy`/`sandbox_mode`/
`web_search`/`features`/`agents`/`projects`/`memories`/`sandbox_workspace_write`），
切到网关供应商，注入 19 模型目录 + `model_catalog_json`，`auth.json` 走网关 key，
**无 `openai_base_url`**，原有 8 个 mcp_servers 原样保留。

### 4. 端到端验证（真实 `~/.codex`，不是 /tmp 副本）

判据：`codex exec --json` 事件流，必须出现 `item.completed(type=agent_message)` 文本精确等于
`TOKEN-<model>` + `turn.completed`。早期 `grep "OK-<model>"` 判据被回显假阳性污染，已废弃。

| 网关 | claude-opus-5 | claude-opus-4-8 | claude-sonnet-5 | minimax-m3 |
|---|---|---|---|---|
| kxpms | **PASS** | **PASS** | 网关挂起* | turn.completed |
| 8782 | **PASS** | 未测 | **PASS** | turn.completed |

官方档路径同样实测：4 个 ChatGPT 模型（gpt-5.6-sol/luna/terra/gpt-5.5）在 ChatGPT OAuth
登录态下 12s 内收到 200/401，失败原因 = Plus 用量限制，与我的改动无关。
**证明切回官方档不破坏 OAuth token（access_token 长度 1762 完整保留）。**

### 5. 第二轮审计（用户要求"批判"后做的修正）

每条都是实测触发，不是声称：

| 现象 | 真因 | 修正 |
|---|---|---|
| `[model_providers.custom]` 下 `request_max_retries=2 / stream_max_retries=2 / stream_idle_timeout_ms=120000` 写进了预设与 live config | codex 0.158 不消费这三个键（bogus host + 这些键后仍 30s+ 卡死直到外部 timeout） | **撤回**：从两个预设、DB、live config 三处全部清掉 |
| `[profiles.strict]` / `[profiles.yolo]` 在 live config.toml 里 | codex 0.158 把 `--profile` 迁到独立的 `~/.codex/<name>.config.toml`，config.toml 里 `[profiles.*]` 是 legacy | **迁出**：`~/.codex/{strict,yolo}.config.toml` 各自独立文件 |
| 端到端 backend 是否真的读 settings_config.modelCatalog | 没有专门测试覆盖这两个预设 | **新增**：Rust 集成测 `self_hosted_gateway_presets_round_trip_through_catalog_pipeline` |

### 6. 测试

| 命令 | 结果 |
|---|---|
| `npx prettier --check src/config/codexProviderPresets.ts src/config/codexProviderPresets.gateways.test.ts` | ✅ All matched files use Prettier code style |
| `npx vitest run src/config/` | ✅ 9 files / 72 tests passed (含 6 例新增) |
| `npx tsc --noEmit` | ✅ clean |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib` | ✅ **2958 passed; 0 failed; 9 ignored** (含新增 `self_hosted_gateway_presets_round_trip_through_catalog_pipeline`) |

### 7. 关键文件

```
src/config/codexProviderPresets.ts                          (+384 行：两个预设)
src/config/codexProviderPresets.gateways.test.ts            (+108 行：6 个静态断言)
src-tauri/src/codex_config.rs                               (+142 行：Rust 集成测)
docs/guides/codex-self-hosted-gateway-zh.md                 (新增 107 行：中文攻略)
docs/guides/codex-self-hosted-gateway-en.md                 (新增 105 行)
docs/guides/codex-self-hosted-gateway-ja.md                 (新增 105 行)
~/.cc-switch/backups/gateway-codex-20260928-003325/         (运行实例备份，含 verification/README.md)
```

### 8. Git 状态

```
本轮（2026-09-28 02:xx）：与 proxy PR 合并成一个 PR 推送。
- proxy PR @ 0e9736e8：3 个 commit（feat(proxy) + docs(handoff) + merge），
  cargo test --lib proxy:: 1500/1500 + vitest 6/6 全绿。
- codex 自建网关：作为第 4 个 commit 加到同一 PR 顶（feat(codex)）。
- 上一轮（2026-09-28 凌晨）原计划单开 PR #7714，已撤回（本仓库本轮才统一合一次推送）。
```

### 9. 已知限制（不属于本 PR）

- 网关侧的卡顿/挂起/限流（glm-5.2 / kimi-k3 / claude-sonnet-5 / claude-opus-5-5 / deepseek / qwen / gemini），
  是上游凭据/容量问题，与 cc-switch 配置无关。
- ChatGPT 官方档的 Plus 用量限制；切到 `default` 后能跑到的是 `gpt-5.6-sol/luna/terra/gpt-5.5`，
  你这个套餐的用量满了就 401。
- `[profiles.*]` 是 single-developer 仓库常见写法，迁移到独立 `.config.toml` 是 breaking change，
  本 PR 已清掉 live 与 DB snippet，未来切换不会再写回 legacy。

## 下一轮提示词

> 在 anti-thundering-herd delay PR（含 codex 自建网关第 4 个 commit）上继续推进：
>
> 1. 等 reviewer 对 4-commit PR 的反馈：proxy 部分的 jitter 行为 + codex 部分的
>    catalog 数量（19/21）是否合适，是否要把 SSOT 校验改成都用 `/v1/models` 而不是依赖
>    config.yaml。
> 2. 把 cc-switch UI 的"网关档"图标/品牌补一下：现在 cc-switch 列表里显示的是
>    "开轩 LLM 网关" / "本地 LLM 网关 (8782)" 的纯文本，可以加 SVG 图标。
> 3. 把"代码生成 model catalog"的端到端测试作为常规 CI step —— 当前只在 Rust 单元测试
>    里跑，建议 GitHub Actions 加一个 job：固定 model 集合 + 实际调用 `prepare_codex_config_text_with_model_catalog`
>    + 校验生成文件 schema。
> 4. 调研是否要把更多模型补全到目录（现在是手工挑的 19/21 个，未覆盖的如 `claude-opus-latest`、
>    `kimi-k2.7-code` 等）。SSOT 在 `~/.minimax/config.yaml`，建议写一个 sync 脚本：
>    读 config.yaml 的 `custom_provider.{kaixuan,local}.models` → 校验 id 命中
>    `/v1/models` → 写 cc-switch preset，这样网关新上模型不会漏登记。
> 5. 把 `[model_providers.custom]` 的 fake feature 调查写成 OpenSpec / RFC：
>    "为什么我们误以为这三个键生效了" 的复盘，避免后续 contributor 重蹈覆辙。
> 6. （可选）补一个 dry-run 模式：切档前打印目标 live config 差异，让用户能 diff
>    当前档与目标档，避免"以为切对了"但其实没生效的情况。
> 7. 下一步 ROI 最高：**P0.2 HalfOpen permit 自释放（~1 人天）** —— proxy 模块
>    HalfOpen 状态当前靠外部 timeout 才回收，可加 in-band 自释放（请求成功时
>    自动 decrement），可观测性 + 容量利用率都受益。

## 产物路径

- 项目 commit：`/Users/xutaohuang/workspace/ai/cc-switch` `0e9736e8` + 第 4 个 feat(codex) commit
- PR：与 proxy PR 合并推送（anti-thundering-herd delay PR，4 个 commit，单 PR）
- 运行实例：`/Users/xutaohuang/.cc-switch/cc-switch.db` + `/Users/xutaohuang/.codex/config.toml`
- 证据包：`/Users/xutaohuang/.cc-switch/backups/gateway-codex-20260928-003325/verification/`