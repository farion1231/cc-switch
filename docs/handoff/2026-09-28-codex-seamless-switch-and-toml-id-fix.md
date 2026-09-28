# Handoff: Codex 切换流程优化 + TOML id 冲突修复（2026-09-28）

## 任务背景

用户在 r0928 末提出 Codex 供应商切换流程的优化需求：

1. **可切换到 OpenAI Official 或开轩 LLM 网关**（kxpms / 本地 8782），
   切换过程「不能导致配置文件出错」。
2. **切换后自动重启 Codex，或无感切换更好** —— 用户明确表示「无缝切换更好」。
3. **kx- 前缀 vs cc-switch 自动路由同模型不同源** —— 用户问要不要给 llm-gateway-go
   的模型加 `kx-` 前缀，或 cc-switch 是否会按相同模型名自动路由。

## 本轮做了什么（按重要性）

### 1. 修复 TOML id 冲突（关键 bug）

**两个端点都用 `model_provider = "custom"` + `[model_providers.custom]`**，
且都没写顶层 `model_provider` 字段。两个并发问题：

- Codex 0.149+ 没有顶层 `model_provider` 时默认回退到 `openai`，导致
  `[model_providers.custom]` 变孤儿表，**CLI 起不来**（实际是连接走错 base_url，
  但表面看像 auth 失败）。
- 即便补了顶层 model_provider，两端共用 `custom` id 也会让
  `merge_inert_codex_provider_tables_into_settings_config` 的
  "live wins" 规则把次写入的整表**静默丢弃**，切回时历史 session 无法 resume
  （RFC 0002 §2.4 rule 1 副作用）。

**修复**：
- `provider_bundle.rs:323-340` —— kxpms 用 `model_provider = "kxpms"` +
  `[model_providers.kxpms]`；local 用 `model_provider = "local8782"` +
  `[model_providers.local8782]`。
- `codexProviderPresets.ts:3394-3411, 3589-3605` —— FE 预设同步切到
  各自的 TOML id。
- `live.rs:3734-3756` —— 测试 fixture 也对齐新 shape。

**新测试**：
- `kaixuan_bundle_distinct_toml_ids_and_top_level_model_provider`：单元测，
  验证两端点 TOML id 不同 + 顶层 model_provider 存在。
- `kaixuan_bundle_inert_merge_preserves_inactive_endpoint_table`：端到端，
  用真实 `kaixuan_bundle()` 输出过 `merge_inert_*`，验证切 kxpms 时
  local8782 的 inert 表被合并进 live（base_url 完整保留），反向同理。
- `kaixuan_bundle_full_switch_lifecycle_no_config_errors`：**真实磁盘**E2E
  测试。装 bundle → 切 kxpms → 切 local8782 → 切回 kxpms → 再切
  local8782，每步核：
    1. live `~/.codex/config.toml` 是合法 TOML
    2. 顶层 `model_provider` 等于目标端点 TOML id
    3. `[model_providers.kxpms]` 和 `[model_providers.local8782]` 两张表
       **都**在 live 里（active + inert 不丢）
    4. 每张表有 `base_url` + `wire_api = "responses"`
    5. `model_catalog_json = "cc-switch-model-catalog.json"` 指针到位
    6. `~/.codex/cc-switch-model-catalog.json` 存在、是合法 JSON、含 8
       个模型（claude-opus-5 / minimax-m3 / glm-5.2 / auto 等必含）
    7. `auth.json` 写的是明文 key（不再含 `$VAR` 字面）
  这条测试是用户要求「确保切换时不会导致配置文件中的错误」的实测
  守护——单测 fixture 守住 inert merge 行为，**真磁盘读写**才是终验。

**额外修复**：跑 E2E 时发现 bundle 的 `modelCatalog` 字段存的是**扁平数组**
`[{model:...}, ...]`，但 `codex_config.rs::codex_catalog_model_specs`（live.rs:1701-1708）
要求 `{models: [...]}` 形态。结果：`prepare_codex_config_text_with_model_catalog`
找不到模型 → 不写 `model_catalog_json` 指针 → Codex `/model` picker 没数据。

修复：把 `provider_bundle.rs:348-368` 的 `model_catalog` 包成 `{models: [...]}`，
对齐生产路径（FE 的 `update_provider` 测试 fixture 与 `managed_provider` 测试
fixture 都用 `{models: [...]}` 形态，provider.rs:1992-2011、mod.rs:3679-3681）。
同步把 `kaixuan_bundle_settings_have_required_keys` 的 `as_array()` 断言
改成 `.modelCatalog.models` 取数组。

### 2. 加 Codex 运行时探测（无缝切换的支撑）

`src-tauri/src/commands/codex_runtime.rs`（新文件）—— 跨平台探测
Codex CLI 进程是否在跑：

- macOS / Linux：`pgrep -x codex`（comm basename 精确匹配，避免误命中
  `codex-config` 等），fallback 到 `ps -A -o comm=`。
- Windows：`tasklist /FI "IMAGENAME eq codex.exe"`。
- 探测在 `tauri::async_runtime::spawn_blocking` 里跑，500ms 阈值，超时
  警告 + 不阻塞 UI。

注册为 `commands::detect_codex_running` Tauri 命令，FE `providersApi.detectCodexRunning()` 暴露。

### 3. 无缝切换的 toast 分态（用户体验）

`useProviderActions.ts:332-407` 在 codex 分支加了**三态 toast**：

- `probeOk && !codexRunning` → 「切换成功。Codex 未在运行，下次启动即可生效」
  （**真正的无缝**：新会话自动用新配置）。
- `probeOk && codexRunning` → 「切换成功。Codex 正在运行：当前会话仍用旧
  配置，新会话已用新配置；点此重启 Codex 让当前会话也生效」+ 「复制重启命令」
  CTA。CTA 不再是占位符——点了之后把可粘贴的 kill 命令写到剪贴板：
  - macOS / Linux：`pkill -f "codex"; sleep 1; echo "..."`
  - Windows：`taskkill /IM codex.exe /T /F 2>nul & echo 重启请运行 codex`
  - 极旧浏览器（无 `navigator.clipboard`）走降级路径：把命令直接贴到
    toast.warning 里给用户复制（8s 显示）。
  - 用户**在终端粘贴**即生效——比 cc-switch 直接 fork+exec 杀进程更安全
    （不会丢未保存的 session 内容，也不会「自己的进程被外人杀掉」反直觉）。
- 探测失败 / 非 codex app → 退回到旧的「切换成功，请重启客户端以生效」
  （保守路径，避免给用户错误的无缝承诺）。

i18n 键（4 个 locale 都加了）：
- `notifications.codexSeamlessSwitched`（未运行）
- `notifications.codexSeamlessSwitchedRunning`（正在运行）
- `notifications.codexRestartCta`（「复制重启命令」按钮文案）
- `notifications.codexRestartCopied`（「已复制重启命令到剪贴板」二次确认）

### 4. 关于 kx- 前缀 vs cc-switch 自动路由（用户提问的回答）

**答：不用 kx- 前缀，用「TOML id 区分」代替；cc-switch 不做同模型跨源路由。**

理由：

- **kx- 前缀会污染模型命名空间**：用户已经在用 `claude-opus-5` 这个 slug，
  加前缀变成 `kx-claude-opus-5`，FE 表 / `/model` picker / 历史 session
  metadata 全要换。无收益（不解决底层问题）。
- **TOML id 区分（已落地）**：`kxpms` vs `local8782` 两个独立
  `[model_providers.*]` 表，session_meta.model_provider 直接引用其中一个，
  Codex 启动时按 id 找表，不再撞路由。
- **cc-switch 当前不做同模型跨源路由**：
  - 模型目录按 active provider 写一次到 `~/.codex/cc-switch-model-catalog.json`，
    切换会**整体覆写**该文件（不是合并）。
  - 切回时再写一份新的覆盖回来——所以「同模型不同源」在 UI 上确实
    看着像「同一个 claude-opus-5 slug 切换后端」。
  - 这是**当前实现的选择**，不是 bug。Codex 的 catalog 模型只服务于
    `/model` picker 的补全 + 档位提示，路由仍由 active provider 决定。

**未来如果要做「同 slug 多端点共存」**（用户提到的「自动路由」）：需要把
catalog 合并写入，slug 加 `@<toml_id>` 后缀（例如 `claude-opus-5@kxpms`
vs `claude-opus-5@local8782`），并加新的 TOML 字段把 slug 反查回
`[model_providers.*]` 表。这是 1-2 天工作量，**本轮不做**。

### 4b. 真机烟雾验证（macOS）

`commands::codex_runtime::tests::detect_codex_running_real_machine_smoke`（`#[ignore]`），
在 r0928 末本机 (macOS) 实测：

- `pgrep -lx codex` → exit 0, stdout `"67526 codex"`（命中 ChatGPT.app
  内置 Codex CLI）
- `ps -A -o comm= | grep '^codex$'` → false（macOS `comm` 列截断，不暴露
  完整 basename，所以 ps fallback 在 macOS 上漏报——这就是为什么我们
  把 pgrep 作为主路径：pgrep 用 `comm basename` 精确匹配，命中正常）
- `detect_unix()` → true（与 pgrep 真值源一致）

结论：本机当前 codex 是「在跑」态，前端 toast 走「Codex 正在运行：当前
会话仍用旧配置，点此重启 Codex 让当前会话也生效」分支，CTA 把可粘贴的
`pkill -f "codex"` 命令写到剪贴板——用户粘贴即生效。

### 5. 关于未来切 minimax / zcode（用户的潜在意图）

当前 cc-switch 的「auto-switch」能力是**故障转移**（auto_failover_enabled），
不是任务路由：

- 已落地：proxy takeover + anti-herd delay + half-open permit + 故障转移队列。
- **没落地**：按任务类型自动挑 provider / 模型。

要做到「minimax / zcode 自动切换」，需要新加一层 routing policy：

- 输入：用户的当前 prompt / session context / 任务分类
- 决策：策略（cost / latency / capability / 用户偏好 / 任务类型）
- 输出：路由到某个 provider + 模型
- 触发：可手动「auto-route」开关，也可结合 session_history 标签

这个独立于本次 codex 切换优化。**本轮未实现**。

## 第二轮（同一文档续写）：真重启 / 自动迁移 / 多端点 catalog / 端口可见

上一节「已知遗留」的 3 条本轮全部处理，外加 handoff「下一轮提示词」的 5 项。

### 1. 真自动重启 Codex（`commands::restart_codex_process`）

CTA 从「把 `pkill` 命令复制到剪贴板」升级为**后端真动手杀进程**。

- `src-tauri/src/commands/codex_runtime.rs` 新增 `restart_codex_process`。
- **不丢上下文红线**：任何 `~/.codex/sessions/**/rollout-*.jsonl` 在
  **30s 内**被写过就**拒绝**重启（`refused = true` + 命中文件路径），要求用户
  先 `/exit`。rollout 目录按 `YYYY/MM/DD/` 分层，探测递归深度 4（Codex 真实
  布局就是 4 层，见 `recent_session_activity_detects_nested_rollout_and_respects_window`）。
- 跨平台终止：macOS/Linux 先 `SIGTERM`，等 2s 仍存活才升级 `SIGKILL`；Windows
  复用 `taskkill /PID <pid> /T /F`（与 `commands::misc::terminate_child_tree`
  同语义，防 `codex app-server` 子进程变孤儿占端口）。
- **整棵树而非只杀根进程**：Codex 0.158+ 是 app-server 架构，CLI 主进程只是
  wrapper，真正占资源/端口的会话在子进程里。用 `pgrep -P <pid>` 逐层展开
  完整后代树后统一发信号。
- **有意偏离：不用 process_group（`kill(-pgid, …)`）**。`terminate_child_tree`
  那样做安全，是因为 cc-switch 自己 `setsid()` 拉子进程、保证它是组长。但这里要杀
  的是**用户从终端启动**的 Codex——它是 shell 作业组的**成员**，组长是那个 shell。
  对非组长 pid 做 `kill(-pid, …)` 要么 ESRCH 失败，要么在 pid 恰好撞上某个存活组
  id 时把用户**整个终端作业组**（连他的 shell）一起干掉。`pgrep -P` 拿到的是确切
  子孙 pid，既能整树清理又完全不碰用户的 shell。
  `process_tree_collection_picks_up_descendants` 里有一条断言专门盯这个红线：
  进程树绝不能包含测试进程自己（即 cc-switch）。
- **只杀不拉起**：Codex CLI 抢 TTY，从 GUI fork 新的会抢焦点也拿不到用户终端
  环境变量。用户回到终端敲 `codex` 即可。
- pid 只来自 `pgrep -x codex` / `tasklist` 的精确名匹配；`parse_pids` 额外过滤
  `pid 0`——`kill(0, SIGTERM)` 是 POSIX 里「向调用者所在进程组发信号」的保留
  写法，混进来等于 cc-switch 给自己发信号。

新增 i18n：`codexRestartDone` / `codexRestartNotRunning` / `codexRestartRefused` /
`codexRestartFailed`。删除死键 `codexRestartCopied`（剪贴板路径已下线）。

### 2. 老用户 TOML 自动迁移（`custom` → `kxpms` / `local8782`）

`codex_config::migrate_legacy_codex_toml_ids`（纯文本函数）+ `..._in_db`（落库）。

- 判定依据是表里的 **`name`**（`kxpms_gateway` / `local_gateway`）而不是表 id，
  迁移目标与 `kaixuan_bundle()` 当前写出的形态**逐字一致**——所以「迁移」与
  「重装 bundle」结果等价。
- 挂在 `write_live_with_common_config_for_codex_oauth_manager`，**每次 codex 切换
  跑一次**；`merge_inert_codex_provider_tables_into_settings_config` 里也做一次
  内存迁移（覆盖 live 文本与每个 DB 行），保证即便 DB 迁移失败本次投影仍正确。
- **失败不阻断切换**：一次 DB 写失败不该让用户连切 provider 都做不到。
- 保守不动（宁可让用户走「重装 bundle」也不弄坏配置）：表 `name` 不是我们的 /
  顶层 `model_provider` 已指向别的 id（用户手工配的第三方路由）/ 目标 id 已被占用。

**踩过的 TOML 陷阱**：`model_provider` 是**标量**，若 `toml_edit::Table::insert`
把它追加到 `[model_providers.*]` 之后，解析回来它就成了那张表的字段，顶层仍空
——Codex 于是回退到 `openai`，老 bug 原样复发。单测用 `toml` crate 重新解析
专门盯这条。

### 3. 同 slug 多端点共存 catalog —— ⚠️ 采集侧已实现，**下发侧默认关闭**（第三轮审计推翻）

这一节在第二轮结束时是**错的**。第三轮批判式审计查证后推翻，结论与证据如下。

**第二轮宣称**：catalog 会生成 `claude-opus-5@kxpms` / `claude-opus-5@local8782`
条目，两端模型在 `/model` picker 里共存。

**审计结论：那是一个功能回归。** 关键事实——

1. **Codex 的 catalog 只有 `slug` 一个身份字段，没有「仅显示的别名」。**
   `codex_catalog_model_entry` 把 `spec.model` 写进 `slug`；而
   `read_codex_model_catalog_simplified_from_live` 又把 `slug` **原样**还原成前端表格的
   `model` 字段（`codex_config.rs` 的 simplified 反解析里就是
   `obj.insert("model", json!(entry["slug"]))`）。那个 `model` 就是请求体里发给
   provider 的模型名。所以 `claude-opus-5@kxpms` 的真实含义是
   「向当前 provider 请求一个叫 `claude-opus-5@kxpms` 的模型」。
2. **按后缀路由的那一半不存在。**
   - Codex 的 `model_provider` 是**配置级全局**，一个 `config.toml` 只能一个 provider，
     配置层表达不了「按模型分发」；
   - cc-switch 侧无任何代码剥离/归一化模型名里的 `@<id>`（仅有两处
     `rsplit_once('@')`，都是解析 URL 的 userinfo，与模型名无关）；
   - llm-gateway-go 侧也没有：`autoroute.promoteCanonical` 用**精确相等**匹配
     `CanonicalName`，带后缀的名字匹配不到任何 candidate。
3. **兜底比报错更危险。** 匹配失败会落到网关的通用评分兜底，于是用户选中
   「local8782 的 claude-opus-5」，实际拿到的是 kxpms 的**某个**模型——看起来
   「能用」，实则静默错路由。这比直接报错糟糕得多。

**修法**：`codex_endpoint_catalog_coexist_enabled()` 总开关**默认关**，
`maybe_merge_endpoint_catalog` 在关闭时原样返回，**绝不写出无法路由的条目**。
采集侧（`MERGED_CATALOG_SOURCES_KEY` + `append_endpoint_suffixed_entries` +
dedup）全部保留并保持测试覆盖，等后缀路由真正落到请求路径上（proxy 侧剥离后缀
并分发，或网关支持该语法）再一行打开。

- 开关：`CC_SWITCH_CODEX_ENDPOINT_CATALOG=1|true|on`。
- `kaixuan_bundle_full_switch_lifecycle_no_config_errors` 的断言随之回退为
  **8 条**（只含 active 端点），并新增一条**红线断言**：写盘 catalog 里出现任何
  带 `@` 的 slug 即判失败。

#### 3.1 采集侧实现要点（仍有效）

- dedup 规则：active 端点条目**原样保留**（顶层 `model_provider` 指向它）；其他
  端点同 slug 加 `@<toml_id>` 后缀；完全重复的最终 slug 只保留第一条。
- **合并进来的模型必须走一遍和 active 相同的展开管线**。来源是前端**简化形态**
  (`{model, displayName, contextWindow}`)，不是 Codex catalog 条目——只改 slug
  的话条目会缺 `base_instructions`（Codex 必填）、工具集、reasoning 档位，在
  `/model` picker 里是个半残壳。`ExpansionSource::{Template, Vendor}` 两条路径
  分别对应中立模板与厂商官方 models.json 镜像。

### 4. 修 `self_hosted_gateway_presets_round_trip_through_catalog_pipeline`（老红转绿）

**根因是 fixture 少模拟一步写盘，不是生产代码有 bug**：
`prepare_codex_config_text_with_model_catalog` 只**返回**加工后的文本，写盘是调用方
`write_codex_live_for_provider` 的职责；而第 4 步的
`read_codex_model_catalog_simplified_from_live()` 读的是**磁盘上的** `~/.codex/config.toml`，
对着空 config 自然解析出 `None`。fixture 补上写盘 + 逐字回读断言即转绿。

顺带把这条测试加 `#[serial]`：它改 `CODEX_HOME`/`CC_SWITCH_TEST_HOME`，而
`get_home_dir()` 读的是 `CC_SWITCH_TEST_HOME`/`HOME`、不是 `CODEX_HOME`——两个都得改，
且不改 `#[serial]` 会和其他改同一批 env var 的测试互相串（这是本轮真实踩到的一次
flaky：新增的 session 探测测试一度让这条老测报 `No such file or directory`）。

### 5. UI 显示实际探测端口

`GatewayEndpointMeta` 新增 `local_port: Option<u16>`（后端从
`KAIXUAN_LOCAL_GATEWAY_PORT` 权威推导）。`KaixuanBundleCard` 里所有硬编码的
「8782」换成实际端口，并在端点行加 `127.0.0.1:<port>` 徽章（带 `data-testid`）。

端口是**运行时**值：用户在 `~/.zshenv` 改了 env var 后界面仍写 8782，会让人以为
「改 env var 没生效」，其实探针早就打在新端口上，只是文案没跟上。

## 本轮测试证据

- `cargo test --lib` —— **3020 passed / 1 failed**。
  唯一红的是 `services::provider::tests::update_current_claude_desktop_provider_syncs_profile_when_proxy_takeover_is_active`，
  与本轮改动**无关**：它 `state.proxy_service.start()` 绑默认端口 15721，而本机
  正在运行的 cc-switch 桌面端（PID 11820）占着这个口 → `Address already in use`。
  已在 `git worktree` 的干净 HEAD 上单独复现，确认是**环境性 pre-existing**，不是回归。
- `cargo test --lib commands::codex_runtime -- --include-ignored` —— 6/6 通过，含真机
  烟雾测试（`pgrep -lx codex` 命中 PID 67526，`detect_unix()` 同步返回 true）。
- 目标项 1:1 对应的 14 条测试单独跑 —— 14/14 通过。
- `npx tsc --noEmit` —— 通过。
- `npx prettier --check` —— 通过。
- 本仓库**没有 eslint 配置**（无 `eslint.config.*`），`npx eslint` 直接报错，属预期，
  不代表代码有问题。

## 第三轮（收口）：遗留清零 + 两个真 bug

上一轮列的 4 条遗留里第 4 条已修；本轮把 kaixuan-bundle handoff 的「下一轮提示词」
一并做完，并在过程中挖出**两个此前一直没被发现的问题**。

### 1. proxy 测试绑死 15721（遗留 #4，已修）

`update_current_claude_desktop_provider_syncs_profile_when_proxy_takeover_is_active`
硬编码默认端口 15721，开发者本地跑着 cc-switch 桌面端时必然
`Address already in use`——不是回归，是**必红**。

改法照抄同文件里已有的正确写法（`listen_port = 0` 让 OS 分配），
并把断言里的 `15721` 换成 `start()` 返回的实际 `info.port`。
注意：**只改端口不够**，断言里还硬编码着 `:15721`，
会从「Address already in use」变成「断言 15721 != 62718」继续红。

### 2. v13 升级路径回归测试（新增，变异验证过）

`migrate_v13_to_v14` 的 `proxy_config_v14` 重建 DDL 里带着
`circuit_half_open_permit_max_age_seconds`。之前只有 fresh-DB 测试覆盖它，
**没有测过真实升级路径**——而这正是老用户升级会走的分支。

新增 `v13_upgrade_path_reaches_current_schema_with_half_open_permit_column`：
fixture 用 **v13 时代真实 DDL**（从 commit `f991726f` 逐字取出），而不是今天的
`create_tables`。两处关键差异让这条测试与 fresh-DB 测试不等价：
v13 的 CHECK 约束里**没有** `'grokbuild'`，也**没有**那一列。

变异验证：手工从 `proxy_config_v14` DDL 删掉该列 → 测试立刻红在
「v13 upgrade must land on a schema that still has ...」，证明不是恒真断言。
（写测试时先踩了一个自己的坑：用裸 `INSERT` 造 `grokbuild` 行会撞 UNIQUE，
因为 `migrate_v13_to_v14` 自己已经插过了——改 `INSERT OR IGNORE`。）

### 3. ⚠️ `codex_config` 测试写进了用户真实的 `~/.codex/`（真 bug，已修）

**这是本轮最重要的发现。** `self_hosted_gateway_presets_round_trip_through_catalog_pipeline`
只设了 `CODEX_HOME`，但**没有任何生产代码读这个变量**：

- `get_codex_config_dir()` = `get_codex_override_dir()`（读 settings 文件）
  → 回退 `get_home_dir().join(".codex")`
- `get_home_dir()` 读的是 `CC_SWITCH_TEST_HOME`

也就是说这条测试一直在写真实用户的 `~/.codex/config.toml`。
实测确认：本机 4 KB 的真实配置（notify / mcp_servers / plugins / projects /
desktop 设置）被替换成了测试 fixture 的 303 字节内容，
`cc-switch-model-catalog.json` 也是测试写进去的。

修复：
1. **先把用户的配置还原**——从 cc-switch 自己的 `proxy_live_backup` 表里取出
   codex 的 `original_config`（JSON 的 `config` 字段），验证 TOML 合法后写回，
   3994 字节 / 11 个顶层键全部回来。
2. 测试改为同时设 `CC_SWITCH_TEST_HOME` + `HOME`（`CODEX_HOME` 保留无害）。
3. 加一条**前置断言**：写任何东西之前先确认
   `get_codex_config_dir() == tempdir/.codex`。将来谁改了路径解析顺序，
   测试会当场失败，而不是继续默默覆盖真实配置。

同类测试 `provider_bundle` / `codex_runtime` 早已正确设了两个变量，所以只有这一条漏了。

### 4. `hermes_config` 跨模块 flaky（真 bug，已修）

`set_provider_preserves_unknown_fields_on_update` 单独跑必过、全量跑偶发红
（`get_provider("acme").unwrap()` 拿到 `None`）。根因不是它自己：

- `get_hermes_dir()` **先**读进程全局 settings 的 `hermes_config_dir`，
  **再**才看 `CC_SWITCH_TEST_HOME`；
- 该测试只有 `#[serial]`，但 `serial_test` 把 `#[serial]` 和 `#[serial(env)]`
  当成**不同的 key**，两组仍然并发跑；
- 于是别的模块测试设的 `hermes_config_dir`（指向一个马上被删的 tempdir）
  会漏进来，`get_hermes_config_path()` 指向一个本测试从没写过的目录。

修法：`with_test_home` 在测试期间把 settings 里的 `hermes_config_dir`
**钉死**到本测试的 hermes 目录，跑完还原。让路径解析自洽，而不是取决于谁最后跑。

### 5. bundle install 真事务化

新增 `Database::install_bundle_endpoints`，把「写 provider + 入队 + sort_index
+ 打开 auto_failover」收进**同一个** `conn.transaction()`。任一步失败整批回滚，
不再有「已写库但未入队」的半成品。重复安装的安全语义不变：同 id 已存在时只刷
membership + sort_index，不覆盖用户改过的 `settings_config`。

回归测试 `install_bundle_endpoints_rolls_back_every_endpoint_on_failure`
用一个必然失败的批次（重复 id 触发 UNIQUE）验证第 1 个端点也被回滚，
并带一条**对照断言**：去掉重复 id 后同一批必须成功写入 2 条——
证明回滚不是因为「这批压根写不进去」。

写这条测试时它立刻抓到我自己的实现回归：existing-row 分支第一版只刷了
`in_failover_queue`，漏了 `sort_index`，把既有的
`install_bundle_preserves_user_prefilled_settings` 打红了。已修（用
`COALESCE(?3, sort_index)` 保住 NULL 语义）。

### 6. 文档补 en/ja

新增 `docs/guides/kaixuan-bundle-en.md` / `-ja.md`，并同步更新 `-zh.md`：
补上「安装是单事务」「端点间切换 / `@` 后缀 catalog 的真实语义」两节
（这两条之前只在 handoff 里，用户文档没有）。

## 本轮测试证据

- `cargo test --lib` —— **3023 passed / 0 failed**（上一轮是 3020/1）。
- `cargo test --lib provider_bundle::tests` —— 18/18。
- `cargo test --lib database::schema::tests` —— 7/7。
- `cargo test --lib hermes_config::tests` —— 57/57。
- 全量跑前后 `md5 ~/.codex/config.toml` 一致 → 真实配置不再被测试污染。
- `npx tsc --noEmit` —— 通过。
- `npx prettier --check docs/guides/kaixuan-bundle-*.md` —— 通过。

## 第三轮：批判式审计（2026-09-28 深夜）

对第二轮自己的产出做查证式复核，**推翻了一条结论**。

### 推翻：item 3「同 slug 多端点共存 catalog」是功能回归

第二轮把它写成「已落地，两端模型在 `/model` picker 共存」。查证后不成立，详见上文
§3。根因是 **Codex 的 `slug` 就是发出去的模型名**（simplified 反解析把 `slug`
原样还原成 `model` 字段），而后缀路由那一半压根不存在（cc-switch 无剥离、网关
`promoteCanonical` 精确相等），匹配失败还会落到评分兜底造成**静默错路由**。

已改为默认关闭下发（`CC_SWITCH_CODEX_ENDPOINT_CATALOG`），采集侧保留 + 测试覆盖。
E2E 断言从 16 条回退为 8 条，并新增「live catalog 不得出现带 `@` 的 slug」红线。

### 修正：item 1 漏杀子进程

第二轮只杀 `pgrep -x codex` 命中的根 pid。Codex 0.158+ 是 app-server 架构，主进程
只是 wrapper，只杀根会留下占资源的孤儿。改为 `pgrep -P` 逐层展开**完整后代树**。

**有意偏离原提示词**：没用 `process_group`（`kill(-pgid, …)`）。`terminate_child_tree`
那样做安全是因为 cc-switch 自己 `setsid()` 拉子进程、保证它是组长；而这里杀的是
**用户从终端启动**的 Codex，它是 shell 作业组的**成员**（组长是 shell），对非组长
pid 做 `kill(-pid, …)` 要么 ESRCH、要么把用户整个终端作业组连 shell 一起干掉。
`process_tree_collection_picks_up_descendants` 有断言盯这条红线：进程树绝不能
包含 cc-switch 自己的 pid。

### 修正：item 2 有一入口没被覆盖

`write_preflighted_or_current_live` 对托管 Codex OAuth provider 直接调
`write_live_snapshot`、**绕过** `merge_inert_*` 与 DB 迁移。把内存迁移补到
`write_live_snapshot` 的 Codex 分支（写盘前最后一道闸），覆盖所有入口。幂等。

### 我的测试曾给一条既有竞态加参与者

新增的 session 探测测试原本改 `CODEX_HOME` + `CC_SWITCH_TEST_HOME`/`HOME`。已改为
**目录注入**（`recent_codex_session_activity_in(&sessions_dir, window, now)`），
完全不碰全局 env——不再给那条竞态添参与者。见下方 flake 数据。

### 并发事故（须知）

审计中途另一个会话把我的分支 fast-forward 进了 `main` 并提交，导致我**未提交的
`codex_config.rs` / `live.rs` 两处改动被冲掉**（一度只剩 worktree 备份可救）。
后续改用 `git worktree` 隔离作业。**教训同前：共享工作区攒未提交改动不可靠。**

## 已知遗留（第三轮之后）

1. **真重启只杀不拉起**（有意为之，见上）。用户需要回终端敲 `codex`。
2. **`local8782` 这个 TOML id 与端口解耦**：`KAIXUAN_LOCAL_GATEWAY_PORT=8899` 时
   TOML id 仍是 `local8782`（bundle 硬编码）。这是**有意的**——改端口不该让历史
   session 的 `session_meta.model_provider` 失效；但 id 字面上带 8782 确实容易误读，
   值得后续考虑改成 `local` 或加注释说明。
3. **共存 catalog 只解决「看得见」，没解决「自动选」**：用户在 `/model` picker 里能
   同时看到 `claude-opus-5` 和 `claude-opus-5@kxpms`，但选中后者不会真的路由到
   kxpms。真正的按端点路由仍需新增 TOML 字段把 slug 反查回 `[model_providers.*]`，
   这部分未做（本轮也不做：需要 Codex 侧配合，不是 cc-switch 单方面能定的）。
4. **`CODEX_HOME` 是个死变量**：`get_codex_config_dir()` 从不读它，但三处测试仍在
   设它。本轮已让唯一真正需要隔离的那条测试不再依赖它；更彻底的做法是删掉这些
   `set_var("CODEX_HOME", ...)`，避免下一个作者再被它误导（这正是本轮这个 bug 的诱因）。
5. **`hermes_config` 的全局 settings 依赖仍在**：本轮用「测试期间钉死
   `hermes_config_dir`」消除了症状，但根因是 settings store 进程全局 +
   `#[serial]` 分组不一致。根治要么给 settings store 加测试隔离层，要么让所有
   相关测试统一用 `#[serial(env)]` 分组。

## Git 状态

- 分支：`feat/codex-restart-toml-migration-merged-catalog`
- 第三轮改动已提交并推送。

### 跨模块 env 竞态（**未修**，独立跟踪）

`openclaw_config::tests::with_test_paths` 报 `written.contains("// top-level comment")`
间歇性失败。**根因不是断言，是进程级 env 的多套互不协调的锁**：

- `CC_SWITCH_TEST_HOME` / `HOME` 是**进程全局**，被多个模块的测试并发改写；
- 保护机制有**三套且互不通信**：`openclaw_config::test_guard()`（模块内
  `OnceLock<Mutex>`）、`hermes_config::test_guard()`（另一把）、以及
  `serial_test::#[serial]`（只协调同样标了 `#[serial]` 的测试，**不会**挡住没标的）；
- 于是一个 `#[serial]` 的 codex 测试与一个用本地 mutex 的 openclaw 测试可以真并发。

**实测**（各 5 次全量 `cargo test --lib`）：
- 干净 `main`（6ca72675）：**3/5 次失败**
- 本分支（+第三轮修正）：**1/5 次失败**

即：**该 flaky 先于本轮存在**，第三轮把我自己的测试移出竞态后频率下降，但**没有
根治**。

**为什么本轮不修**：涉及 `openclaw_config`(4 处) / `hermes_config`(2 处) /
`codex_config`(4 处) / `provider_bundle`(14 处) 共约 24 个 env 改写点、4 个模块，
需要引入一把**全 crate 共享**的 env 锁并统一改造。改动面大、且与并发会话在同一批
文件上作业，风险高于收益。**建议单开一个 issue/PR 专门做**，修法：新增
`crate::test_support::env_lock()` 全局 `OnceLock<Mutex<()>>`，所有改
`CC_SWITCH_TEST_HOME`/`HOME`/`CODEX_HOME` 的测试统一持有，并让两处 `test_guard()`
改为取同一把锁。

