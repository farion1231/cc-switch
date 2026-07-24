# CC Switch Repository Memory

## 2026-07-25 Codex official 模型图片能力丢失根因

- 症状：在 CCSwitchMulti MultiRouter 下选择 official GPT 模型时，Codex Desktop 提示模型不支持图片输入。
- 根因：官方原始 `~/.codex/models_cache.cc-switch-backup.json` 中 `gpt-5.6-sol/terra/luna/gpt-5.5/gpt-5.4/gpt-5.4-mini` 都带 `input_modalities=["text","image"]` 和 `supports_image_detail_original=true`，但 CCSM 的 `FetchedModel` 只承载 `id/owned_by/context_window`。OAuth/cache 动态目录同步到 provider、route、聚合 catalog 时丢失模态字段；随后 `CodexCatalogToolProfile::NativeResponses` 用 text-only 模板生成 `cc-switch-model-catalog.json`，所有 official GPT 被写成 `input_modalities=["text"]`。
- 修复边界：后端 `codex_oauth_models` 从在线响应和本地 cache 解析 `input_modalities/inputModalities/modalities.input` 与 `supportsImage/supports_image/vision/supports_image_detail_original`；前端 `FetchedModel`、Workspace 刷新、MultiRouter 向导合并都保留 `inputModalities` 与 `supportsImage`；最终 `codex_config` 生成 NativeResponses catalog 时依据这些字段覆盖 text-only 模板。Spark/DeepSeek V4 这类明确 text-only 的模型仍保持文本能力，不能被模板或 route 误开启图片。
- 验证：定向 Rust 测试覆盖 OAuth 图像能力解析与 NativeResponses catalog 图文输出；前端 `codexMultiRouterWizard`/`codexMultiRouterSync` 单线程测试、`pnpm typecheck`、`cargo fmt --check`、`git diff --check` 通过。真实 `~/.codex` 未写入；安装新构建后需刷新/重建 MultiRouter official catalog，再重启 Codex app-server 读取新的 `cc-switch-model-catalog.json`。

## 2026-07-25 Codex Desktop 新版历史修复根修

- 最新 Desktop 的 `thread/list` 以 active `~/.codex/state_5.sqlite` 的 `threads` 为主，且 `backfill_state.status='complete'` 会阻止 rollout 再扫描；只改 `model_provider`、`session_index.jsonl` 或 mtime 会让 CCSM 自己的 JSONL 列表看似正常，但 Desktop 仍会因 `has_user_event`、`preview`、`first_user_message`、`recency_at_ms` 缺失而隐藏会话。
- 修复入口必须在 Codex/ChatGPT 完全退出时，以 `sessions` 与 `archived_sessions` 的 rollout 为事实来源重建/插入 `threads` 元数据，并在同一 SQLite 事务更新 `backfill_state`；`session_index`、workspace hint 和项目映射只能作为辅助，绝不能写 `local_thread_catalog`。
- 当前 rollout 的真实用户输入结构是 `type='event_msg'` 且 `payload.type='user_message'`，不能继续用旧的字符串或顶层 `type='user_message'` 判断。preview 与 first_user_message 仅从此真实事件或目标事件 preview 提取，避免编造可见历史。
- 写入前继续拒绝运行中的 `ChatGPT.exe` / `OpenAI.Codex` / `codex.exe app-server`，并备份 state DB（含 WAL/SHM）和受影响 rollout；修复后需重启 Desktop 验证历史不再闪现后消失。
- `Continue in a new task` 会把父会话的 `session_meta.id` 原样复制到多个独立 rollout；这些文件的 `rollout-<timestamp>-<uuid>.jsonl` UUID 才是分支物理身份。历史重建必须只在声明 ID 冲突时改用该 filename UUID，分别写入每个 `threads.id -> rollout_path`，不能再用 `BTreeMap<session_meta.id, ...>` 覆盖不同进度的分支，也不能改写原始 JSONL 的 parent ID。

## 2026-07-20 本机构建缓存与旧发布输出清理

- 清理前仓库最大的可再生成项是 `src-tauri/target`：`52264183717` bytes（48.675 GiB），其中 debug 约 39.011 GiB、release 约 9.663 GiB；现场没有进程从该目录运行，因此可整目录删除，后续 Cargo/Tauri 构建会按需重建。
- 同步删除可再生成的 `node_modules`（约 278 MiB）、`dist`（约 5 MiB）和未跟踪的 `scripts/logs`（约 318 MiB）。pnpm 的链接节点可能让第一次递归删除报 access denied；部分删除完成后再次对已核验的 `node_modules` 根目录执行目录删除即可，不要扩大到 pnpm 全局 store。
- 删除 `output/release-v3.16.4-4-upload`、`output/release-v3.16.4-5wizard`、`output/release-v3.16.5-3` 三组已过期本地发布产物，合计约 210 MiB；保留 `output/pdf/codex-multirouter-guide-zh.pdf`。清理后工作区除 `.git` 外约 54 MiB。
- 清理前后均保留用户未提交的 `src-tauri/src/proxy/forwarder.rs`、`src-tauri/src/proxy/providers/codex.rs`、`src-tauri/src/proxy/providers/mod.rs` 修改。以后做空间回收应先检查 `git status` 和运行进程，再只删 target、dist、node_modules、旧 release output、诊断 logs 这类可再生成或已交付内容。

## 2026-07-15 Windows WM_ENDSESSION tao Destroyed panic 根修

- 用户报告的 `app-exit-events.jsonl` 在 2026-07-15 11:18:55 记录主线程 panic：`tao-0.34.6/src/platform_impl/windows/event_loop/runner.rs:371 cannot move state from Destroyed`。同一次运行在 11:18:44 托盘 `show_main` 后发生，但 `show_main` 只会增加重绘消息，不能产生 `Destroyed`；没有 `ExitRequested` 日志也排除了 CCSwitchMulti 主动退出路径。
- 根因来自 tao 0.34.3 引入的 Windows `WM_ENDSESSION(TRUE)` 处理：它在 `DispatchMessageW` 回调内部直接调用 `loop_destroyed()`，而外层 `GetMessageW` 循环仍存活；随后已排队的 `WM_PAINT`、内部 `PROCESS_NEW_EVENTS` 或 user event 会再次调用 `poll/send_event/redraw`，从终态 `Destroyed` 迁移并 panic。该逻辑在 tao 0.34.6、0.34.8、0.35.0 和当时 dev 分支仍存在，单纯升级不能修复。
- 根修保留 tao 0.34.6 其余上游修复，并通过 `src-tauri/Cargo.toml [patch.crates-io]` 固定仓库内 `src-tauri/vendor/tao`：`WM_ENDSESSION(TRUE)` 改为 `PostQuitMessage(0)`，让下一次 `GetMessageW` 返回 0，之后只由 `run_return` 的正常尾部发送一次 `LoopDestroyed`；`WM_ENDSESSION(FALSE)` 继续运行。严禁用 catch panic、忽略 Destroyed 状态迁移或仅把 panic 改成 return，这些做法会留下半销毁事件循环。
- CCSwitchMulti 在最终 `RunEvent::Exit` 调用 `app_exit_monitor::record_clean_exit("event_loop_exit", 0)`，使系统关机、重启、注销或 Restart Manager 关闭后的下一次启动不再误报残留 marker；用户主动退出仍走原有 `ExitRequested` 清理路径。
- 回归基线：vendored tao 的 `confirmed_end_session_quits_the_message_loop` / `cancelled_end_session_keeps_the_message_loop_alive` 两个单测、完整 `cargo check --manifest-path src-tauri/Cargo.toml --lib`，并确认构建输出明确解析 `tao v0.34.6 (.../src-tauri/vendor/tao)`。上游发布等价修复前不得删除 vendor override。

## 2026-07-15 CCSwitchMulti v3.16.5-10：数据库 v13 兼容与白屏恢复

- 已发布 `v3.16.5-9` 只支持 schema v12；上游 `f991726f`（2026-07-11）首次把数据库升至 v13。用户曾运行上游 v3.17.0 后再回到 v3.16.5-9，会在启动预检中进入“数据库版本过新”恢复页。
- 正式兼容修复只回移 schema 子集，不整包 cherry-pick `f991726f` 的 usage/proxy 统计逻辑：`SCHEMA_VERSION=13`、新库两张 usage 表增加 `input_token_semantics`、v12 -> v13 用幂等 `add_column_if_missing` 补列。现有 v13 多出的列不会破坏旧查询；默认值 0 保留旧或未知语义。严禁通过手改 `PRAGMA user_version` 伪造降级。
- v3.16.5-10 同时包含 `2990291e` 的异常 MultiRouter 目录过滤和 `31a541d5` 的根错误边界。前者修复打开旧方案时 `modelCatalog.models` 的 null/字符串条目导致的 TypeError；后者把未捕获 React 渲染异常转为可见恢复页，而不是纯白。
- 发布前必须运行 v12 -> v13 迁移单测、MultiRouter/错误边界 Vitest、TypeScript、Prettier、rustfmt、`cargo check --lib`、生产 renderer 构建与 `git diff --check`；不要纳入历史 `output/`、`scripts/logs/` 或用户未归属的本地输出。

## 2026-07-15 CCSwitchMulti 数据库恢复页与纯白窗口的分流排查

- 必须区分两条故障链：截图中的 `v13 > v12` 是数据库恢复页，不是前端白屏；恢复页由 2026-06-24 的 `e93f7cab` 引入，`v3.16.5-9` 已包含它。因此同版本若窗口纯白且没有恢复页，不能再笼统归因数据库版本过新。
- 已发布 `v3.16.5-9` 中可确认的 MultiRouter 纯白根因来自 `8afa3fb2093c57710b6b63d433a29b8d327fc817`（2026-07-01，`fix(codex): separate multirouter catalog and subagent ordering`）：向导初始化直接执行 `existingPlan.settingsConfig.modelCatalog.models.map(model => model.model)`。历史方案若含 `null`、字符串等非对象条目会抛 `TypeError`；根组件此前没有 React ErrorBoundary，最终只显示纯白。
- `2990291e` 修复该已确认路径：向导仅在打开时挂载，`readWizardModelCatalog()` 过滤非对象、null、空模型名，已有方案初始化复用安全读取，未知状态机步骤回退首步。该修复解释“打开/创建 MultiRouter 或复用旧方案后白”，但不能证明真正空数据目录的首次启动白屏由此引起；空目录没有异常 `modelCatalog`，仍须按该用户的日志和数据定向复现。
- 新增 `AppErrorBoundary` 并在 `main.tsx` 包裹正常 App 根树。后续渲染/React 提交阶段异常不再退化成空白，而显示可重新加载、可提供错误详情的恢复页，并在控制台记录组件栈。异步网络/事件错误仍应由各调用点处理。
- 回归：`tests/lib/codexMultiRouterWizard.test.ts` 覆盖 null、字符串、空 model 条目；`tests/components/CodexMultiRouterWizard.test.tsx` 覆盖异常历史目录；`tests/components/AppErrorBoundary.test.tsx` 覆盖子树渲染异常显示恢复页。首次启动仍白的用户需要收集 `%USERPROFILE%\\.cc-switch\\logs\\cc-switch.log`、应用版本与脱敏 provider 数据后再归因，不能混同为 v13 数据库问题。

## 2026-07-14 GitHub Issue #12/#15/#16 triage

- Issue #15 报告版本为 `v3.16.5-2`。MultiRouter 不按会话固定 Provider；每次请求都会根据当前 body 中的 `model` 重新解析 route。`v3.16.5-5` 已修复模型目录重复、未选 Provider 注入和新模型元数据同步，因此先要求升级 `v3.16.5-7`、完整重启 Codex Desktop，并提供同一 trace 的 `route_resolved`、`upstream_status`、`upstream_error` 和切换前后模型名。没有这组证据时，不删除跨 route fallback，也不把客户端缓存猜测当成根因。
- Issue #16 的 OMP 是 Oh My Pi，主要使用 `~/.omp/agent/config.yml` 和项目 `.omp/config.yml`；Pi 使用 `~/.pi/agent/settings.json`，但自定义 Provider 的正式扩展点是 TypeScript extension `pi.registerProvider()`。两者不能共用普通 JSON 写入器；正式支持应拆成 OMP YAML/JSONC adapter 与 Pi extension generator/manager，上层再抽象 provider projection target。
- Issue #12 截图中的真实错误是商汤上游 `HTTP 400: invalid_tool_call_id`，不是泛化的“DeepSeek 不会调用工具”。上游同源问题为 `farion1231/cc-switch#4056`，且已有报告称商汤直连也失败。修复前必须拿到同一 trace 中“上游返回 tool call id”和“下一轮回传 tool_call_id”的脱敏对照，才能判断长度/字符集限制、上游自相矛盾还是 Responses 与 Chat 转换改写；不能盲目重写 ID 破坏多轮映射。
- 本轮已回复并保持打开：`BigStrongSun/ccswitchmulti#15`、`#16`、`#12`。#15/#12 等待最新版复测证据，#16 作为 enhancement 等待正式双 adapter 实现。

## 2026-07-13 Codex OAuth originator：白名单保留与缺失回退

- `originator=codex_cli_rs` 能修复 Luna 相对 `cc-switch` 的模型准入差异，但不能因此把所有真实 Desktop/VS Code 请求都改报为 CLI。官方 Codex 的 first-party 分类包含 `codex_cli_rs`、`codex-tui`、`codex_vscode`、`Codex ` 前缀，以及独立 chat 分类中的 `codex_atlas`、`codex_chatgpt_desktop`；`codex_exec` 不在该分类中。
- 根修位于 `src-tauri/src/proxy/forwarder.rs`：可信本地 Codex 请求只有在恰好携带一个白名单值时保留；缺失、未知、重复值统一回退 `codex_cli_rs`。External API 和 Claude/协议转换不能保留调用方 originator，避免外部请求伪造 first-party 身份。
- 可信来源不靠可伪造的 header 猜测：`RequestContext::new` 仅为本地应用入口设置 `preserve_codex_client_originator=true`，`new_with_provider` 的 External API 临时 provider 固定为 false，再由 `RequestForwarder` 执行最终 header 规则。`handle_responses` 同步改用 `should_handle_as_codex_client()`，确保 External API 标记/API key 优先于伪装成 Codex 的 User-Agent。
- 回归测试必须覆盖 CLI、VS Code、TUI、Desktop/chat first-party 保留，`cc-switch`、`codex_exec`、未知值、重复值和 External API 回退，以及非 OAuth 请求完全不改写。

## 2026-07-13 CCSwitchMulti v3.16.5-7：所有官方 Codex OAuth 模型请求统一原生 CLI originator

- 真实根因：Luna 的 `prefer_websockets=true` 只是传输偏好，不是 WebSocket 硬要求。原生 Codex CLI `0.144.2` 在 `supports_websockets=false` 下可通过 HTTP Responses 正常运行 Luna。
- 受控实测：使用相同 OAuth token、ChatGPT account、`/backend-api/codex/responses` endpoint、请求体和其余 headers，仅切换 `originator`。`gpt-5.6-luna` 使用 `codex_cli_rs` 返回 HTTP 200 和 `OK`，使用 `cc-switch` 返回 HTTP 404 `Model not found`；`gpt-5.6-sol` 使用 `codex_cli_rs` 同样返回 HTTP 200。因此问题是官方后端按客户端来源做模型准入，不是临时网络错误或 HTTP 不支持 Luna。
- 修复边界：所有由 CCSwitchMulti 解析为 `AuthStrategy::CodexOAuth` 的模型推理请求都在最终 header 合并与 Provider override 之后强制写成单一 `originator: codex_cli_rs`。普通 Responses、raw passthrough、Claude 转 Codex OAuth 都覆盖；第三方 Bearer/API Key 请求不改写。
- 目录修复：删除 `merge_codex_models()` 对 `prefer_websockets` 的过滤，Luna 可继续出现在已路由 catalog 中，不映射到 Terra/Sol。真正不可用模型仍由官方模型列表的 `supported_in_api: false` 过滤。
- 共享规则：`CODEX_OAUTH_ORIGINATOR` 是托管 OAuth 模型请求唯一来源常量；Codex/Claude adapter 只生成 Authorization，避免先追加错误来源头再形成多值。官方 OAuth 模型列表请求同步使用该常量，订阅/额度接口仍保留独立的 `Codex Desktop` 身份。
- 发布约束：只修改仓库源码、测试、文档和版本，不原子替换、不构建安装包、不停止或覆盖当前 `3.16.5-6` live 程序。版本统一提升为 `3.16.5-7` 后提交并通过 GitHub release workflow 发布。

## 2026-07-13 Codex 用量页：官方窗口预测与本地 token 分析分离

- `src/components/codex/CodexUsagePage.tsx` 的官方 `SubscriptionQuota` 仅提供窗口已用百分比 `utilization` 与 `resetsAt`，不会提供绝对 token 上限或模型级官方额度账本。因此绝不能把本地 token 直接换算成“官方还剩多少 token”。
- 页面现在以当前窗口的已过时间和官方百分比计算“百分点/小时”的平均速度，并仅在该假设下预测耗尽时刻。预测会显示中/低置信度及“按当前节奏可能提前耗尽”建议；窗口刚开始或数据异常时降级为等待更多读数，不伪造精确性。
- `useUsageSummary({ preset: "today" }, { appType: "codex" })` 与 `useModelStats({ preset: "7d" }, { appType: "codex" })` 是独立的本地日志口径：显示今日 token、token/小时、缓存命中率、成功率和前四模型分布。其数据来自本机代理/会话同步日志，仅用于分析节奏和模型结构。
- 顶部刷新同时刷新官方订阅窗口、本地今日汇总和七日模型统计；页面仍保持只读，不兑换 reset、不修改账号或 Codex 配置。
- 验证基线：`pnpm vitest run src/components/codex/CodexUsagePage.test.tsx`、`pnpm typecheck`、`pnpm build:renderer`、Prettier 检查和 `git diff --check` 都应通过。单独 Vite 预览没有 Tauri `invoke` 桥接，不能作为真实数据页的端到端验证；需在 `pnpm tauri dev` 或桌面壳中继续做 live QA。

## 2026-07-12 MultiRouter OAuth 模型列表网络失败兜底

- 截图里的 `Request failed: error sending request for url (https://chatgpt.com/backend-api/codex/models?client_version=...)` 来自 `src-tauri/src/services/codex_oauth_models.rs` 的 `reqwest.send()` 阶段，说明请求还没拿到 HTTP 响应；这不是模型名格式、401/403、HTTP 404 或接口返回空列表，而是 DNS/TLS/代理/超时等网络层问题。
- 修复边界：OAuth 在线模型列表失败时，MultiRouter 向导和 Routes 工作台会读取本地 Codex 官方模型缓存兜底，优先 `~/.codex/models_cache.cc-switch-backup.json`，再读 `~/.codex/models_cache.json`；缓存解析只保留 `gpt-*`、`codex-*`、`chatgpt-*`、`o*` 这类官方模型，避免把 CCSM 合并进 cache 的 Qwen/DeepSeek 写进 official route。
- 向导默认模型源会收敛等价 OAuth provider：未绑定账号的 `default` 与 `codex-official` 代表同一个默认 ChatGPT 账号，只保留已有真实 catalog 或稳定 seed 中更合适的一个；不同 `accountId` 的 OAuth provider 不合并。
- 后端 OAuth 模型请求错误现在会展开 reqwest 错误分类、底层 source 链和 CCSwitchMulti 全局代理状态，便于根据用户截图判断是 timeout/connect/TLS/proxy，而不是只看到模糊的 `error sending request`。

## 2026-07-11 CCSwitchMulti v3.16.5-2 Release

- `v3.16.5-2` 已作为 `BigStrongSun/ccswitchmulti` 正式 release 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.5-2`。Release 为 `draft=false`、`prerelease=false`，发布工作流 run `29158855394` 全部成功。
- 本次 tag `v3.16.5-2` 指向 annotated tag object `a0e8bf94381952848e6d24ff7549febf82da99fe`，解引用提交为 `61e025999942e356f080cc80fd7512ab9570f4b7`；`fork/main` 也已推进到同一提交。
- Release 资产共 19 个，包含 Windows x64/ARM64 setup 与 portable、macOS dmg/tar.gz/signature/zip、Linux x64/ARM64 AppImage/signature/deb/rpm，以及 `latest.json`。
- `latest.json` 下载验证通过：`version=3.16.5-2`，updater 平台包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64`，每个平台都有 signature 和 release asset URL。
- 发布前本地固定交付目录 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti` 已导出 Windows 测试包，metadata 指向二进制修复提交 `08b58d81707a9458cd2f4c99f06f9aafcc60b416`；随后只追加了 release note 提交 `61e02599`，不影响二进制内容。

## 2026-07-11 Codex Image Gen MultiRouter 官方兜底边界

- “内置 Image Gen 在 MultiRouter 下 404”的第一层根因是旧版本没有注册 `POST /images/generations` / `/v1/images/generations` 等 Images API 路由；请求一旦打到 `127.0.0.1:15721/v1/images/generations`，会先在 Axum 层 404，还没进入 MultiRouter route resolver。
- “之前不影响”不能理解成 proxy 里曾经有隐藏的图片绕过逻辑。现场更合理的边界是当时 Codex/ChatGPT App 没把 Image Gen 走本地 base_url，或已经切回 official 直连官方；通用 `forwarder` 一直会对所有 Codex endpoint 按 `body.model` 解析 MultiRouter。
- Images API 的修复必须是 endpoint 专用：如果用户显式把 `gpt-image-*` route 到第三方 Images provider，就让通用 router 继续处理；如果没有显式图片 route，则按 route 身份扫描 enabled routes，物化 official OAuth route，避免 `defaultRouteId` 把图片请求发给 DeepSeek/Qwen 等文本 provider。
- 按身份兜底命中的 official route 要清理 `codexResolvedUpstreamModelOverride`，不能把图片模型改成 `gpt-5.5` 这类文本模型；同时把 `codexResolvedRouteMatched=false` 写进 request-local provider，日志和后续转换才能区分“图片原生能力回官方”和普通模型命中。
- 回归测试固定三条边界：旧 router 中 official route 只匹配 `gpt-5.x` 仍可承接 `gpt-image-*`；`defaultRouteId` 指向 DeepSeek/Qwen 时图片不应落到非官方默认路由；显式第三方图片 route 不能被强制改回 official。
- 2026-07-11 后续修正：unknown `/v1/*` 不能再返回本地 404/501 或依赖逐个 endpoint allowlist。Axum fallback 应进入 raw passthrough：只解析请求体副本用于 MultiRouter 选路，上游仍使用原始 method/path/query/body/content-type，并由 forwarder 重建 Host、Content-Length、Authorization、API key、external API key 等敏感/链路头。已注册的 `/v1/responses`、`/v1/chat/completions`、`/v1/images/generations` 仍走专用 handler。
- raw passthrough 的 MultiRouter 选路和 Responses 不同：显式模型 route 命中优先；没有显式命中时只能找 official/Codex OAuth route 身份，找不到 official 就视为配置缺失，不能再退到 defaultRouteId/首个 enabled route。这样所有未知 GPT App 原生请求默认回官方，只有明确配置到 Qwen/DeepSeek 等第三方 provider 的模型才走其它链路。

## 2026-07-11 Codex 历史修复面板单确认流

- `CodexHistoryRepairPanel` 的用户主流程应保持为“刷新记录 -> 选择 session/项目范围 -> 选择目标 provider -> 确认修复”。不要再把“修复新版 App 历史 / 加载历史 / 预览旧版恢复 / 写入旧版索引”四个并列按钮暴露给用户，否则会把新版目录同步和旧版离线索引兜底混成两套互相竞争的修复逻辑。
- 项目路径默认必须为空且不参与查询/写入，代表跨项目读取和修复所有匹配记录；只有用户勾选“只修复单个项目”后，项目路径才会传给 `list_codex_history_sessions` / `repair_codex_history_visibility`。打开面板时传入的 `initialProjectPath` 只能作为“带入当前项目”的快捷值。
- “确认修复”必须每次先 dry-run，再弹出明确的离线写入确认：要求用户完全退出 Codex / ChatGPT App，说明运行中的 app-server 会覆盖修复结果；用户确认后才执行真实写入。后端的进程保护错误也要改写成同样的可执行提示。
- 目标 provider 下拉应包含“当前 provider”（前端传 `targetProvider: null`，由后端按 live config/codex_model_router_v2 解析）、`openai`、`custom` 和 active DB 扫到的所有 provider 桶，并在选项中展示 live/config/db 计数线索。
- `unlock_codex_model_picker` / “启动并刷新新版目录”只是写入成功后新版侧边栏仍未重建时的高级兜底，不应放在主操作区。Codex 目录、Active DB、archived/subagent 开关也属于高级设置，默认界面只保留范围和 provider 选择。

## 2026-07-11 Codex 新版历史修复路径与旧版索引兜底边界

- 新版 Codex/ChatGPT App 历史修复主路径不是改 `state_5.sqlite`：`CodexHistoryRepairPanel` 的“修复新版 App 历史”只调用 `unlock_codex_model_picker`，通过 CDP 注入 renderer 后执行 App 自己的 `localThreadCatalog.requestStartupSync()`。它不依赖“加载历史/预览旧版恢复”，也不应写 `threads.model_provider`、rollout、`session_index.jsonl` 或 `.codex-global-state.json`。
- 旧版“预览/写入旧版索引”是离线兜底：会读取 active DB、枚举所有非目标 provider 桶，必要时改 `threads.model_provider`、`has_user_event`、聚焦时间、rollout 首行、`session_index.jsonl`、workspace hints 和 rollout mtime。写入前必须确认 Codex/GPT App 退出，避免 app-server/WAL 或新版目录同步覆盖结果。
- 历史修复面板默认不应从当前会话详情继承项目路径；空项目路径才代表跨项目读取/修复。`initialProjectPath` 只适合作为“带入当前项目”的手动快捷项，否则会让用户误以为 SQLite 历史只限当前项目。
- provider 桶候选应来自 live config、active DB 全量 `threads.model_provider` 分布、常见 `openai/custom/codex_model_router_v2` 兜底，并在 UI 里显示计数/来源。下拉关闭时只显示当前目标项，不代表只读到了这个桶。
- `codex_desktop::install_script` 必须用 `Runtime.evaluate` 返回值提取 `historySync`；`Page.addScriptToEvaluateOnNewDocument` 只返回脚本 identifier。若解析错对象，新版历史同步会显示未请求或一直等待 renderer target，即使当前页面脚本实际已执行。

## 2026-07-10 Codex 官方回退残留本地代理与不可用 OAuth 模型过滤

- 现场截图里的 `gpt-5.6-luna` 404 不是 MultiRouter 路由漏匹配：`codex-router.log` 显示请求已命中 OpenAI Official route 并转到 `https://chatgpt.com/backend-api/codex/responses`，最终由官方后端返回 `Model not found`。OAuth 模型目录同步不能把官方返回但显式标记 `supported_in_api=false`、`available=false`、`enabled=false`、`disabled=true` 或 `visibility/status/availability=hidden|disabled|unavailable|unsupported|denied` 的条目写进 provider catalog。
- 一键切回 OpenAI 官方必须清掉 live `~/.codex/config.toml` 顶层 provider-owned 字段：`base_url`、`wire_api`、`openai_base_url`、`experimental_bearer_token`、`model_catalog_json`、`model`、`model_provider` 及活动自定义 `[model_providers.*]`。只清 `openai_base_url` 不够；新 Codex Responses 会读顶层 `base_url = "http://127.0.0.1:15721/v1"`，导致 UI 显示 official 但请求仍走 CCSwitchMulti 本地代理。
- `switch_codex_to_official_and_repair_history` 不能在最前面用历史离线检查阻断官方回退。Codex/ChatGPT App 运行时，应先退出本地代理并把 live provider 强制设为 `openai`，然后把历史修复作为可跳过 warning 返回；否则用户点击“一键切回”会因为历史锁定失败而继续滞留在本地代理。
- common config/provider live merge 也要把 `base_url`、`wire_api` 视为 provider-owned 本地代理字段，避免后续导入配置片段又把 15721 代理注入 official live config。`model_context_window` 仍属于用户偏好，应继续保留。

## 2026-07-10 新版 GPT App 线程与多 Agent 机制取证

- 完整实测报告见 `docs/guides/gpt-app-thread-subagent-mechanism-2026-07-10.md`。Windows 新桌面包为 `OpenAI.Codex`，实际运行链是 `ChatGPT.exe -> resources/codex.exe app-server -> code-mode/tool runtimes`；历史的原始事件在 `~/.codex/sessions/**/rollout-*.jsonl`，`state_5.sqlite` 提供 thread 元数据和 `thread_spawn_edges` 父子关系。修复历史必须离线备份并把 JSONL、SQLite 与子线程 edge 当成整体，不能只改轻量 `session_index.jsonl` 或伪造 `local_thread_catalog`。
- 新版 multi-agent 的当前 live 保留工具契约为 `collaboration.spawn_agent(task_name, message, fork_turns)`；历史 v1 的 `model/reasoning_effort/service_tier` 与历史 v2 的可选 `agent_type` 绝不可重新硬编码。截图的 `reserved for use by this model and must match configured schema` 根因是旧 CCSwitchMulti 扩展了保留 schema；`ensure_codex_multi_agent_reserved_schema_compatible` 必须保持 `hide_spawn_agent_metadata=true`，子 Agent 模型改由 `~/.codex/agents/*.toml` role 选择。
- 角色、模型、推理强度、provider 路由和 `spawnAgentModels` 前五候选是不同层。当前 Codex 内置角色为 `default/worker/explorer`；custom role 可 pin `model`/`model_reasoning_effort`，缺省继承父会话。前五候选仅影响 picker/CCSwitchMulti role 投影，不改实际路由或服务端最终调度。本机 5.6 已进总目录且有历史子线程使用；没有出现在特定候选卡片是旧人工前五被保留，需手动排进前五保存。
- 官方依据：`https://learn.chatgpt.com/docs/agent-configuration/subagents`、`https://learn.chatgpt.com/docs/app-server`。本机当前 config 为 `agents.max_threads=10`、`max_depth=1`；实际运行并发还需取产品会话资源上限与该配置的最小值。写密集任务仍需串行或 worktree 隔离，独立 Agent thread 不等于自动独立文件系统。

## 2026-07-10 MultiRouter 长模型摘要换行

- 规则列表的模型摘要位于 `src/components/codex/CodexRouterWorkspacePage.tsx` 的 `RouteListButton`。此前卡片没有 `min-w-0/w-full` 约束，摘要还使用 `truncate`；官方 OAuth 同步到多个 GPT-5.6 模型后，左侧规则列无法收缩，文本会覆盖右侧详情面板。
- 根修是让卡片收缩到父级 grid 单元，并将摘要改为 `whitespace-normal break-words leading-5`，保留完整模型名并在卡片内换行。该改动不改变路由匹配或模型目录数据。
- UI 修复后 `pnpm exec prettier --check src/components/codex/CodexRouterWorkspacePage.tsx`、`pnpm typecheck` 和 `src/components/codex/CodexRouterWorkspacePage.test.ts`（40 项）通过。

## 2026-07-10 Legacy MultiRouter OAuth 5.6 Migration

- `a3e4622f` 等动态 OAuth catalog 修复只覆盖带 `targetProviderId` 的新式 route；现场数据库里两个 OpenAI Official provider 和新 `codex-multirouter` 已有三个 GPT-5.6，但截图选中的旧 `codex-openai-router` 仍是无 `targetProviderId` 的内联 `managed_codex_oauth` route，因此 Workspace 的 provider-id 预过滤和同步 helper 都会跳过它。
- 根修位于 `src/lib/codexMultiRouterSync.ts`：同步前迁移 legacy OAuth route。有 `accountId` 时只允许匹配同账号 provider；默认账号优先稳定 id `codex-official`，不存在 canonical 时只有唯一的无账号 OAuth provider 才可迁移，避免多账号静默串号。迁移后复用既有 route/modelMap/聚合 catalog SSOT 同步，重复执行必须幂等。
- `CodexRouterWorkspacePage` 刷新 provider 后不再提前按 `targetProviderId` 排除 plan，而是让同步层逐个返回 changed/null；这是旧 route 能在首次官方 catalog 刷新时被持久化迁移的必要条件。
- 子 Agent `spawnAgentModels` 仍按原设计保留用户前五排序；GPT-5.6 会进入 route 和聚合 catalog，并出现在“路由/全部”候选池，但不会自动挤掉用户手工候选。回归覆盖 canonical 默认账号、精确多账号匹配、账号无匹配保护、幂等和 Workspace 实际持久化；同步 9 项、Workspace 40 项及 `pnpm typecheck` 通过。
- 本地测试包版本推进到 `3.16.5-2`，四个版本面为 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`，用于与已安装的 `3.16.5-1` 明确区分。

## 2026-07-10 Codex 一键切回 OpenAI 官方与全量历史归桶

- Codex 首页的官方恢复入口放在 `ProviderList.tsx` 底部 CTA 区，与“配置多路模型”并排。它不能复用普通前端 provider switch：接管态下普通路径有官方供应商安全门，专用命令 `switch_codex_to_official_and_repair_history` 才能把“退出接管、保留 OAuth、切官方、修历史”收束为一次操作。
- 数据库官方 seed id 是 `codex-official`，Codex live `config.toml` 和会话历史使用的官方 provider id 是 `openai`，二者不能混用。专用命令会确保 seed 存在，调用现有 `ProviderService::switch` 保留 `auth.json`，再显式把 live `model_provider` 设为 `openai`；provider 字段清理必须保留 MCP、memories、projects、插件等全局配置。
- “统一 Codex 会话历史”开启时，官方配置仍可能被注入 `model_provider=custom`。一键切官方必须同步关闭 `unify_codex_session_history`、清空 `unify_codex_migrate_existing` 和 `codex_official_history_unify_v1` marker；如果 provider 切换失败，应恢复操作前 settings。
- 官方历史修复只改 rollout `session_meta.model_provider` 与 SQLite `threads.model_provider`，把所有非 `openai`（包括未知、空值和缺失值）归并到 `openai`；不要复用完整 visibility repair 去改标题、时间、聚焦、session index 或全局工作区状态。JSONL 与 state DB 写入前分别备份，重复执行必须为零改动。
- 新版 Windows App 的主进程名是 `ChatGPT.exe`，安装路径/命令行包含 `OpenAI.Codex_*`，并启动 `codex.exe ... app-server`。任何配置或历史写入之前必须用现有进程探测拒绝运行中的 App；本机验证该过滤器能同时命中 `ChatGPT.exe` 与 app-server。新版 App 的 `local_thread_catalog` 是派生数据，不要直接改，下一次启动交给现有 native sync 重建。
- 验证覆盖：配置转换单测通过；`codex_history_migration::tests` 40 项通过（含未知 provider 全量归桶和幂等）；`ProviderList` 6 项通过并覆盖空/非空列表按钮；`cargo check` 与 `pnpm typecheck` 通过。真实用户数据迁移未在本轮执行，因为新版 ChatGPT App 正在运行，离线保护应先拒绝而不是自动关闭用户进程。

## 2026-07-10 MultiRouter OAuth GPT-5.6 Catalog Boundary

- 当前官方 ChatGPT/Codex OAuth 模型端点已经能返回 `gpt-5.6-sol`、`gpt-5.6-terra`、`gpt-5.6-luna`。本机使用现有 OAuth 登录态只在内存中发起 `GET https://chatgpt.com/backend-api/codex/models?client_version=3.16.5-1`，返回 HTTP 200 和 8 个模型；凭据未输出。`~/.codex/models_cache.json` 也已有三个 5.6 模型，但 `~/.codex/cc-switch-model-catalog.json` 与数据库里两套 MultiRouter plan 仍只有 `gpt-5.5`、`gpt-5.4`、`gpt-5.4-mini`、`gpt-5.3-codex-spark` 四个官方模型。
- 根因是 MultiRouter 接线缺失，不是 OAuth 上游缺模型。后端 `src-tauri/src/services/codex_oauth_models.rs`、`src-tauri/src/commands/codex_oauth.rs` 和前端 `src/lib/api/model-fetch.ts` 已实现 `get_codex_oauth_models` / `fetchCodexOauthModels`；但 `src/lib/codexMultiRouterWizard.ts` 和 `src/components/codex/CodexRouterWorkspacePage.tsx` 各自写死旧四模型 fallback，`CodexMultiRouterWizard.tsx` 刷新时又明确把 OAuth 标成 `skipped` 并 `continue`，因此真实 OAuth 目录从未写入 provider、route match 或聚合 catalog。
- 转发层没有把模型限制到 5.5：官方 route 使用 `gpt` 前缀匹配，Rust resolver 按 exact/prefix 路由。只要 5.6 被写入 MultiRouter catalog/route，路由可以进入托管 OAuth；最终准入仍由账号和官方后端决定。另有旧默认目录散落在 `src-tauri/src/proxy/handlers.rs` 与 `src-tauri/src/proxy/external_openai_api.rs`，会影响本地/对外 `/v1/models` fallback。
- 正确根修应让 MultiRouter 向导和 Routes 页对 OAuth 源调用现有 `fetchCodexOauthModels(accountId)`，成功后持久化并同步 provider catalog、route match、聚合 catalog；网络/认证失败时才保留旧目录作为 fallback。同时收敛重复的前后端默认模型常量，避免下一次新模型发布再次漂移。远端 `fork/main`、`origin/main` 与当前 `codex/merge-upstream-v3.16.5`（`d9658086`）均尚无这项修复。
- 运行态附带发现：当前安装运行的是 `3.16.4-15`，不是仓库/构建目录的 `3.16.5-1`；`15721` 当前未监听且 Codex proxy/takeover 关闭，只有 external API `15722` 在监听。因此“当前界面拿不到 5.6”既有静态目录根因，也不能用当前 15721 做真实路由验证。
- 修复已在同一分支完成：`a3e4622f` 将向导和 Routes 工作台接到 `fetchCodexOauthModels(accountId)`，成功后追加并持久化官方新模型，再复用 `syncCodexMultiRouterPlanWithProviders` 同步 route match 与聚合 catalog；空列表、认证或网络失败保留最后一次可用目录。`04c52606` 删除后端 External API 的旧四模型运行时注入，并让临时 `codex-official` provider 只增加 `provider_type=codex_oauth`、不再覆盖动态 `settings_config`。最终审查又发现“从未保存 catalog 的 OAuth provider”仍会先物化旧四模型再与在线结果合并，`ba739bd9` 因此彻底删除向导和 Workspace 的静态 fallback：新 OAuth provider 在首次成功获取前保持空目录，失败时只保留真实持久化过的历史目录，不再凭空声明账号可用模型。
- 回归验证：MultiRouter 向导/Workspace/同步 helper 共 84 项前端测试通过，覆盖绑定 OAuth accountId、`gpt-5.6-sol` 写入 provider/route/catalog、失败保留旧目录；`pnpm typecheck`、Prettier、Rustfmt、`cargo check` 通过；Rust External models 4 项、动态 catalog 保留 1 项、空 OAuth seed 1 项、OAuth 模型解析 5 项通过。工作树中另有未提交的 `codex_config.rs`、`codex_desktop.rs`、`codex_history_migration.rs`、`CodexHistoryRepairPanel.tsx` 修改，本次均未纳入提交。

## 2026-07-10 CCSwitchMulti v3.16.5-1 Release Boundary

- `v3.16.5-1` 是 CCSwitchMulti 对原版 `cc-switch v3.16.5` 的跟进发布版本；正式 tag 应使用 fork tag `v3.16.5-1`，不要复用上游 `v3.16.5` tag。`release.yml` 会读取 `docs/release-notes/v3.16.5-1-zh.md` 生成 GitHub Release 正文，资产外显命名继续使用 `CCSwitchMulti-${TAG}-...`。
- 本版必须在 release note 顶部注明：**尚未适配 OpenAI 新版 ChatGPT 应用**。当前合并只覆盖原版 `cc-switch v3.16.5` 与 CCSwitchMulti 既有 Codex Desktop / CLI 接管路径；如果新版 ChatGPT 应用改变桌面端协议、模型菜单、渲染器门控或登录态结构，需要后续单独适配。
- 发布前不要提交未跟踪的本地输出目录：`output/release-v3.16.4-4-upload/`、`output/release-v3.16.4-5wizard/`、`scripts/logs/`。发布流程仍是先把当前分支 fast-forward 推到 `fork/main`，再推 annotated tag 触发 `.github/workflows/release.yml`。
- `v3.16.5-1` 已作为 BigStrongSun/ccswitchmulti 正式 release 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.5-1`。Release 为 `draft=false`、`prerelease=false`，发布时间 `2026-07-10T05:41:45Z`，正文确认包含“尚未适配 OpenAI 新版 ChatGPT 应用”提示。
- GitHub Actions release run `29070736037` 全部成功：Windows x64/ARM64、macOS universal、Linux x64/ARM64、Publish GitHub Release、Assemble `latest.json` 均为 success。Release 资产共 19 个，外显命名为 `CCSwitchMulti-v3.16.5-1-*`，包含 `latest.json`。
- 下载验证 `latest.json`：`version=3.16.5-1`，`pub_date=2026-07-10T05:41:56Z`，updater 平台包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64`。

## 2026-07-10 Upstream v3.16.5 Merge Into CCSwitchMulti

- 本次上游跟进合并原版 `farion1231/cc-switch` 的 `v3.16.5` 功能到 CCSwitchMulti，版本面统一推进为 `3.16.5-1`：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 都必须保持一致；Tauri `productName` 仍是 `CCSwitchMulti`，identifier 仍是 `com.ccswitchmulti.desktop`，不要被原版品牌回退覆盖。
- Codex 原生 Responses 预设现在可以带 `modelCatalog` 作为模型能力/上下文窗口目录，但这不等于强制开启 Codex `/model` 菜单映射；单 provider 菜单投射仍由 `meta.codexLocalModelMapping` 控制，MultiRouter routes 开启时才走聚合投射。Chat Completions provider 仍需要本地路由/接管转换，不能把 native Responses 直连语义套到 Chat 桥接路径。
- 上游新增的 native Responses catalog profile 字段要在前端编辑往返中保留：`supportsParallelToolCalls`、`inputModalities`、`baseInstructions` 属于隐藏能力字段；`useCodexConfigState.extractCodexCatalogModels` 不能在字段缺失时写入空字符串形式的 `upstreamModel`、`displayName`、`contextWindow`，否则保存后会污染原生 catalog 行并破坏测试里的隐藏字段往返。
- `CodexCatalogToolProfile` 这类 catalog 能力配置应从 provider 的 `apiFormat` 推导：`openai_responses` 走原生 Responses profile，`openai_chat` 走 Chat 转换 profile。不要用 `modelCatalog` 是否存在来推断 wire API，也不要让 catalog 元数据反向改变 provider 的实际协议。
- 官方 OAuth/auth 边界继续保持：CCSwitchMulti 的接管和路由配置不应覆盖用户 `auth.json` 的 ChatGPT OAuth 登录态；official/OAuth route 仍由 managed auth path 物化，第三方 API Key 放 provider-scoped config 或本地代理占位，不要写进官方登录态。
- 本轮验证覆盖前端 catalog/session/preset 目标测试、JSON locale parse、Prettier、`pnpm typecheck`、Rust `cargo fmt --check`、Codex/provider Rust 单测、`cargo check` 和 `git diff --cached --check`；未跟踪的 `output/release-v3.16.4-4-upload/`、`output/release-v3.16.4-5wizard/`、`scripts/logs/` 属于既有本地输出，合并提交不纳入。

## 2026-07-09 CCSwitchMulti v3.16.4-16 Release

- `v3.16.4-16` 已作为 `BigStrongSun/ccswitchmulti` 正式 release 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-16`。Release 为 `draft=false`、`prerelease=false`，GitHub latest API 返回 `tag_name=v3.16.4-16`，发布时间为 `2026-07-09T13:06:57Z`。
- Release 提交为 `413b2699b90f459b888858b230ffd7d63526727d`（`chore(release): bump CCSwitchMulti to v3.16.4-16`），annotated tag `v3.16.4-16` 解引用到同一提交；`fork/main` 已 fast-forward 到该提交。Tag 推送后第一次 Actions run `29016634597` attempt 1 只有 Linux x64 成功，其余 matrix job 在未执行 step 前被 GitHub 标记 cancelled，直接 rerun 同一 run 后 attempt 2 全部成功。
- GitHub Actions release run `29016634597` attempt 2 覆盖 macOS universal、Linux x64/ARM64、Windows x64/ARM64、Publish GitHub Release 和 Assemble `latest.json`，所有 job 均为 success。Release 资产共 19 个，外显命名均为 `CCSwitchMulti-v3.16.4-16-*`，包含 Windows x64/ARM64 setup 与 portable、macOS dmg/tar.gz/signature/zip、Linux x64/ARM64 AppImage/signature/deb/rpm，以及 `latest.json`。
- 远端 `latest.json` 已下载验证：`version=3.16.4-16`，包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64` 6 个 updater 平台，且每个平台 signature 字段都存在。
- 本地固定交付目录 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti` 已由 release pipeline 刷新到 `Version: 3.16.4-16`、`Commit: 413b2699b90f459b888858b230ffd7d63526727d`，包含 Windows x64 NSIS setup、setup `.sig`、portable zip、raw exe、`latest.json` 和 `SHA256SUMS.txt`。发布前本地验证覆盖 Qwen/vLLM Rust 单测、Responses->Chat Rust 单测、ProviderForm vitest、Prettier、`pnpm typecheck`、`cargo fmt --check`、`git diff --check` 和 `pnpm release:local`；仅有既有 browserslist/baseline/chunk/Tauri `__TAURI_BUNDLE_TYPE` 警告。

## 2026-07-09 Codex Responses Missing Output Budget Semantics

- 最新纠偏：这不是 Qwen/vLLM 专用问题，而是 Codex Responses -> Chat Completions 转换必须保留“未声明输出上限”的通用语义。Codex 原生 Responses 请求当前通常不写 `max_output_tokens/max_tokens/max_completion_tokens`；CCSwitchMulti 只有在原请求显式携带这些字段、provider 显式配置 `defaultOutputTokens`、或 Anthropic thinking-budget 错误重试整流时，才应让上游 Chat 请求出现 max token 字段。
- 当前 live 日志样本里，Qwen route 的 `request_shape.top_keys` 没有 `max_tokens/max_completion_tokens/max_output_tokens`，因此该样本不能再归因为“CCSM 当前自动补 32768 导致截断”。历史自动写入 `defaultOutputTokens=32768` 的代码路径确实会改变缺省语义，已改为不再由 Qwen/vLLM 推断或前端保存自动生成。
- 正确验证口径：当 Codex 没发输出预算且 provider 没有显式 `defaultOutputTokens` 时，转换后的 Chat 请求也不应出现 `max_tokens` 或 `max_completion_tokens`；`minOutputTokens` 只允许抬高已经存在且过小的显式预算。若 live 日志仍显示旧 `thinking` / `reasoning_effort` 形态，要单独判断是运行二进制未更新、DB 旧 meta 未被合并修正，还是该上游本身接受该字段，不能混同为输出上限截断。

## 2026-07-09 Retire Qwen vLLM Implicit Output Cap

- 用户指出 `Codex 没发输出长度，vLLM 也没要求输出长度` 时，CCSwitchMulti 自动补 `defaultOutputTokens=32768` 会把本应由上游决定的输出长度截断。更准确地说：这是历史自动默认值会改变 Codex 缺省语义的风险，不应作为 Qwen/vLLM 推断默认值，只应保留为高级用户显式配置能力。
- 修复边界：Qwen + vLLM/matrixminecraft 推断仍设置 `thinkingParam=enable_thinking`、`effortParam=none` 和 `minOutputTokens=2048`，但 `minOutputTokens` 只抬高已经存在且过小的 `max_tokens/max_completion_tokens`，请求完全缺省时不再注入任何输出预算字段。
- 旧版自动写入的 Qwen/vLLM `defaultOutputTokens=32768` 现在视为废弃自动值，在运行时合并 Qwen/vLLM reasoning meta 时清掉；显式非 32768 的 `defaultOutputTokens` 继续保留。前端 ProviderForm 保存 Qwen/vLLM meta 时也不再自动写 `defaultOutputTokens=32768`。

## 2026-07-09 Codex Public Evidence For Output Budget And Subagent Reasoning

- 公开核验结论需要修正表述：`defaultOutputTokens=32768` 不是“从根上修 Codex 客户端”，也不应简单说成“规避 Codex 弱点”。OpenAI Codex 当前原生 Responses 请求结构本身没有 `max_output_tokens` 字段，`build_responses_request` 也不写输出预算；公开 issue `openai/codex#4138` 报告 `model_max_output_tokens` 不进入 Responses 请求，`#31181` 也把“Codex native wire_api=responses 不发送 max_output_tokens”当作与 OpenAI-compatible gateway 兼容的证据。
- 对 CCSwitchMulti 更准确的边界是：当 Codex 的 Responses 风格请求被 CCSM 转成 Chat Completions 发给 Qwen/vLLM 时，不能因为 Codex 原生缺省输出预算就替用户补一个默认上限。`defaultOutputTokens` 应只代表用户显式配置的默认上限，不再由 Qwen/vLLM 推断分支自动生成。
- 关于子 Agent reasoning：官方 subagents 文档确认当前 Codex 默认启用 subagent，且自定义 agent 文件可以包含 `model`、`model_reasoning_effort` 等普通 config 键；这些字段省略时继承父会话。因此 CCSM 旧 Qwen role 硬编码 `model_reasoning_effort="low"` 会覆盖父会话/模型默认，移除该字段让 Qwen 继承是正确方向。公开 issue `openai/codex#27712` 也把 role 中 `model` / `model_reasoning_effort` 会进入 child request body 当作预期并写了验证说明。
- 公开搜索没有找到“reasoning-only stream 必然导致 subagent 卡住”的官方确认 issue。相关公开证据只有 `openai/codex#30179`：Codex backend 可先流 `response.reasoning_summary_text.delta` / keepalive，而最终 `response.output_text.delta` 可能集中成一个大块返回；这支持“Codex 子 Agent 进度/可见输出可能长时间不可见”的风险判断，但不足以证明 upstream Codex 已确认有 reasoning-only stream 卡死 bug。后续排查必须继续以本机 `codex-router.log` 的 `done_seen/finish_reasons/client_disconnected/request_shape` 和 Qwen 上游实际 SSE 为准。

## 2026-07-09 Qwen Codex Subagent Context And Output Budget Boundary

- Qwen3.6 Local 的 Codex 可见上下文当前是 262144：`~/.codex/config.toml` 的 provider model entry、`~/.codex/cc-switch-model-catalog.json`、`~/.codex/models_cache.json` 都写了 `contextWindow/context_window=262144`；顶层 `model_context_window=272000` 只是兜底，不是 Qwen route 的实际 catalog 值。
- 真实上游 `https://www.matrixminecraft.cn:24443/vllm/v1/models` 只返回 `id=qwen3.6`、`owned_by=vllm`、`max_model_len=262144` 等字段，未返回 `max_output_tokens/max_tokens/maxOutputTokens`。CCSwitchMulti 的 `FetchedModel` 也只保存 `id/owned_by/context_window`，Codex catalog 类型也没有输出上限字段，所以“云端给了 max output 但 CCSM 丢了”当前没有证据；准确说是上游 `/models` 未给，CCSM schema 也未承载。
- Qwen 子 Agent 失败链路更像输出预算/思考参数配置缺口，不是 context window 错配：`codex-qwen-local` meta 显式写了 `codexChatReasoning`，但没有 `minOutputTokens`，并且 `thinkingParam` 是 `thinking`；`resolve_codex_chat_reasoning_config` 看到显式 meta 会直接返回，绕过 Qwen vLLM 推断分支。推断分支本来会给 matrixminecraft/vLLM Qwen 设置 `thinkingParam=enable_thinking` 和 `min_output_tokens=2048`。
- 运行日志 `codex-router.log` 中 Qwen 请求已正确命中 `router-codex-qwen-local` 并发往 `/chat/completions`，多次 `upstream_status=200/response_ready=200`；`request_shape.top_keys` 只有 `messages,model,parallel_tool_calls,reasoning_effort,stream,stream_options,thinking,tool_choice,tools`，没有 `max_tokens/max_output_tokens`。注意当前日志摘要不会单独打印 max token 字段值，但字段若存在会出现在 `top_keys`。
- 修复落点：`resolve_codex_chat_reasoning_config` 现在会在识别到 Qwen + vLLM/matrixminecraft 推断结果时，对旧的显式 `codexChatReasoning` 做定向兼容：`thinkingParam=thinking` 或缺省会改回 `enable_thinking`，缺失或小于 2048 的 `minOutputTokens` 会抬到 2048，但 DeepSeek/OpenRouter/SiliconFlow 等非 Qwen/vLLM 显式配置仍保持覆盖。`ProviderForm` 保存时也必须保留 `minOutputTokens`，并在 Qwen vLLM provider 上写出同一组安全默认值，防止 DB 再生成“显式但不完整”的 reasoning meta。
- 仍不把 catalog 级 `max_output_tokens` 纳入本次修复：当前证据是上游 `/models` 未给可靠输出上限，CCSM schema 也未承载；如后续要做，应作为独立模型能力 schema 任务处理。
- 更正：`minOutputTokens` 只能解决“Codex 明确传了过小输出预算”的截断；当 Codex 子 Agent 请求完全没有 `max_tokens/max_completion_tokens/max_output_tokens` 时，CCSwitchMulti 不应代填 `max_tokens`。Qwen + vLLM/matrixminecraft 推断不再写 `defaultOutputTokens=32768`，旧版自动值会在运行时清掉；显式小预算仍由 `minOutputTokens=2048` 抬高，显式非 32768 的 `defaultOutputTokens` 保持不覆盖。
- Qwen 子 Agent 的 `reasoning_effort=low` 根因是 CCSwitchMulti 托管 `~/.codex/agents/qwen-local.toml`/`ccswitch-qwen-local.toml` 曾经硬编码 `model_reasoning_effort = "low"`，会覆盖 catalog 的 `defaultReasoningEffort` 和用户后来在 Codex 配置里调的 high/xhigh。新生成的 Qwen 托管 role 不再写 `model_reasoning_effort`，让 Codex 按当前会话/模型默认继承；Spark/DeepSeek 等角色仍按各自语义写 low/medium/high。
- `codex-router.log` 的 `request_shape` 现在会显式记录 `max_tokens/max_completion_tokens/max_output_tokens` 字段形态；修复后验证缺省 Codex live 请求时，不应看到 `max_tokens` 或 `max_completion_tokens`，除非原请求或 provider 显式配置了输出预算。如果日志仍显示 `enable_thinking=absent`、`thinking=object(keys=[type])`，需要单独排查运行中的 CCSwitchMulti 二进制/服务是否已重启到新代码或 DB 旧 meta 是否被合并修正。

## 2026-07-09 Codex Desktop Model Picker And Managed Subagent Roles

- `BigStrongSun/ccswitchmulti#10` 的截图中 `remote_debugging_enabled=false`、`remote_debugging_port=None`、`model_catalog_models=Some(12)` 是关键组合：catalog 已生成，但 Codex Desktop 以普通方式启动，renderer 侧 Statsig `107580212` 仍可能把模型菜单压回“自定义”或少数官方模型。排查时先让用户完全退出 Codex Desktop，再从 CCSwitchMulti 点“解锁模型菜单”用 CDP 端口启动和注入，不要先把问题归因为 catalog 缺失。
- 上游 `farion1231/cc-switch#4169/#5066/#4420` 同类反馈可作为背景：无官方 ChatGPT/Codex 登录态时 Desktop 模型选择器本身会门控自定义模型；CCSwitchMulti 的可控修复边界是保留官方登录态、写 `model_catalog_json`/inline models/cache、以及 CDP 注入 renderer 白名单。
- 新版 Codex 子 Agent 不只看 `spawn_agent` 工具说明里的前 5 个 picker-visible 模型，还会读取 `~/.codex/agents/*.toml` custom agent role。CCSwitchMulti 生成的 role 文件带 `# Managed by CCSwitchMulti. Do not edit this file by hand.` 标记；同步当前前 5 个候选时必须删除不再属于当前候选窗口的托管 role，否则 Codex 智能体列表会继续显示历史自定义模型。
- 不要把 `service_tiers=[]` 直接当成自定义模型不可见的根因。Codex core 和 TUI 测试里空 `service_tiers` 是合法模型路径；如果要补 `available_in_plans`、`minimal_client_version` 等新字段，必须先用当前 Codex Desktop/app-server 版本验证字段过滤逻辑，不能仅根据 issue 评论猜测写模板。
- 链路状态页的“解锁模型菜单”是用户处理 renderer 白名单门控的显式下一步：按钮 hover/`title` 必须说明它会通过 remote debugging 启动或连接 Codex Desktop 并注入 renderer 模型白名单，不会改路由、API Key 或模型目录；页面也要短提示“先完全退出 Codex Desktop，再点击解锁”，避免用户只看到“自定义”菜单却不知道要触发该动作。
- 模型菜单解锁会在 Codex 接管开启或确认已接管后 best-effort 自动尝试一次；它不是常驻守护修复。若 Codex Desktop 已经以普通方式运行且没有 CDP 端口，CCSwitchMulti 不会静默杀进程或重启用户窗口，必须提示用户完全退出 Codex Desktop 后再点“解锁模型菜单”。
- `#10` 的 portable 前提指 CCSwitchMulti portable，不是 Codex Desktop portable。不要恢复前端“手动选择 Codex.exe”主流程，也不要把路径写进 `ccswitch.codexDesktopExecutablePath`；正确边界是后端在用户点“解锁模型菜单”且发现正在运行的 Desktop shell 路径时，把该路径记到 `.cc-switch/codex-desktop-executable.json`，用于后续复用已校验路径或覆盖非标准安装发现失败。
- Codex Desktop、Codex CLI 和 app-server 要分层处理但都要支持：大写 `Codex.exe` 是 Desktop/Electron shell，用于 renderer/CDP 模型菜单解锁；小写 `codex.exe` 是 CLI/app-server，`codex app-server` 是 JSON-RPC 协议服务，修复路径是 live `config.toml`、`model_catalog_json`、`models_cache.json`、本地 `/v1/models` 和 MultiRouter 转发，不能当成 Desktop renderer 可执行文件启动。
- CLI catalog 验证和 Desktop 模型菜单解锁是两条路径：`codex debug models` / `/v1/models` 用来证明 CLI 或 app-server 能看到原始模型目录；Desktop 菜单仍可能被 renderer/Statsig/React cache 过滤，需要 CDP 注入白名单。修 issue #10 时两边都要看，但不要把 CLI 成功等同于 Desktop 菜单已解锁。
- Codex Desktop 自动发现应由后端完成：优先复用正在运行的大写 `Codex.exe`，再通过 WindowsApps 包目录和 `Get-AppxPackage -Name OpenAI.Codex` 的 `InstallLocation` 查找 MSIX 安装位置；错误文案必须明确提示安装/启动 Codex Windows app，并说明 CLI/app-server `codex.exe` 不能解锁 Desktop renderer。
- 多用户报 `Codex Desktop executable was not found` 时，根因通常在 Desktop shell 路径发现覆盖面而不是模型目录：非管理员进程可能不能枚举 `C:\Program Files\WindowsApps`，包名也可能不只是 `OpenAI.Codex`。自动发现必须依次覆盖运行中进程 `ExecutablePath`、记住的后端路径、App Paths 注册表、PATH 中大写 `Codex.exe`、`Get-AppxPackage -Name *Codex*` / best-effort `-AllUsers`、AppxManifest.xml 的 `Application Executable`、常见 `%LOCALAPPDATA%\Programs` / `%ProgramFiles%` / scoop 路径；这里排除小写 `codex.exe` 只表示不能把 CLI/app-server 当 Desktop/CDP 启动目标，不表示 CCSwitchMulti 不支持 CLI。
- Windows `Get-CimInstance Win32_Process -Filter "Name = 'Codex.exe'"` 是大小写不敏感匹配，本机会同时返回大写 Desktop `app\Codex.exe` 和小写 app-server `app\resources\codex.exe app-server --analytics-default-enabled`。运行中 Desktop 进程检测必须再对 `ExecutablePath` 的 leaf 做 `-ceq 'Codex.exe'` 精确过滤，否则可能把 CLI/app-server 误当成 Desktop 主程序；CLI/app-server 的可用性要看 `codex debug models`、`model/list`、`/v1/models` 和 live config/catalog 证据。
- macOS/Linux 不能复用 Windows 的 `Codex.exe` 校验：macOS Desktop shell 是 `Codex.app/Contents/MacOS/Codex`，CLI 仍是小写 `codex`；Linux 先保守接受大写 `Codex` 或 `Codex*.AppImage`，不要把 PATH 里的小写 CLI 当 Desktop。非 Windows 首次发现需要覆盖运行中 Desktop、已记住路径、macOS `/Applications` / `~/Applications` / Spotlight `Codex.app`，以及 Linux `Codex`/`Codex.AppImage` PATH、`.desktop` 绝对 `Exec` 和常见 `/opt` / `~/.local/bin` / `~/Applications` 路径。
- API Key 登录不会让 MultiRouter 路由或 CDP 注入本身失效：第三方 key 应放在 provider-scoped `config.toml` token/本地代理占位符里，`auth.json` 的 ChatGPT OAuth 登录态应被保留；接管生成的本地 provider 继续带 `requires_openai_auth=true`，注入脚本还会把 renderer auth context 临时修成 `chatgpt`。但如果用户从未有官方 ChatGPT/OAuth 登录态，或 Codex Desktop 后续登录/刷新重建 renderer 状态，原生模型菜单仍可能重新回到“自定义”，需要完全退出 Desktop 后再次点“解锁模型菜单”。不要把这种展示层回退误判成上游 API Key 不能中转。
- “完全退出再点”不是第三方 API Key 登录后的常规动作，只针对当前 Desktop 已经普通启动且无 CDP 的状态。成功由 CCSwitchMulti 带 CDP 启动并注入后，切换 DeepSeek/中转站 API Key provider 不应重复解锁；只有 Desktop 进程被用户关闭、普通方式重开、或 renderer 重建导致注入脚本丢失时才需要再次触发。遇到“解锁模型菜单报错”先看是否找不到已安装或已校验的大写 `Codex.exe`，再看 CDP 端口/renderer target，不要直接归因于 OAuth 或 API Key。

## 2026-07-09 OpenClaw Default Model Catalog Canonicalization

- 用户排查 OpenClaw 不支持流式/思考等级时，只读确认 WSL `~/.openclaw/openclaw.json` 中 `models.providers.vllm.models[0]` 已有 `reasoning=false`、`input=["text","image"]`、`contextWindow=128000`、`maxTokens=8192`；因此“思考等级不可用”不是 OpenClaw 完全没读模型声明，而是 live 配置把该模型声明为不支持 reasoning。
- 同一份 live 配置还暴露了默认模型引用不一致：`agents.defaults.models` 是 `vllm/Qwen3.6`，但 `agents.defaults.model.primary` 是 `vllm/qwen3.6`。这类大小写/目录 key 不一致会让 OpenClaw 的模型能力表现得像没读到。
- 根修边界在 `src-tauri/src/openclaw_config.rs::set_default_model`：设置 `agents.defaults.model` 时必须同步把 primary/fallback refs 写入 `agents.defaults.models`，并把仅大小写不同的旧 catalog key 迁移到 canonical ref，保留旧 entry 的 alias/extra。不要只在 ProviderList 的“一键设默认”按钮里补前端逻辑，否则其他命令入口仍会写出不自洽配置。
- 前端缓存边界：`src/hooks/useProviderActions.ts::setAsDefaultModel` 成功后需要同时 invalidate `openclawKeys.defaultModel`、`openclawKeys.agentsDefaults` 和 `openclawKeys.health`，否则 Agents 面板可能继续显示旧的 catalog/default 状态。
- 回归测试：`default_model_write_registers_catalog_refs`、`default_model_write_canonicalizes_case_variant_catalog_ref`、既有 `default_model_write_preserves_top_level_comments`，以及 `tests/hooks/useProviderActions.test.tsx`。验证命令：`cargo test --manifest-path src-tauri/Cargo.toml default_model_write --lib`、`pnpm vitest run tests/hooks/useProviderActions.test.tsx`、`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`pnpm typecheck`。

## 2026-07-08 CCSwitchMulti Release Asset Name Branding Repair

- 用户问“为什么最新的 release 里应用名的 multi 没了”。事实边界：GitHub latest release `v3.16.4-15` 的标题是 `CCSwitchMulti v3.16.4-15`，但远端资产名和下载说明是 `CC-Switch-v3.16.4-15-*`；本地固定目录 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti` 的 Windows 导出仍是 `CCSwitchMulti_3.16.4-15_x64-setup.exe`。
- 根因不在 Tauri 产品配置：`v3.16.4-15` 的 `src-tauri/tauri.conf.json` 仍为 `productName: "CCSwitchMulti"`、`identifier: "com.ccswitchmulti.desktop"`，`package.json` 仍为 `cc-switch-multi`。根因在 `.github/workflows/release.yml` 的 release stage 手写资产重命名模板，沿用了上游 `CC-Switch-${VERSION}-...`、`CC Switch.app` 和 `CC Switch` DMG volname。
- 历史修复分支 `d78622ed chore: update release asset names` 只存在于 `fork/codex/codex-icon-refresh`，不是 `main` 祖先。不要直接 cherry-pick 它的全部内容，因为它把 Windows portable 搜索路径改为 `ccswitchmulti.exe`；当前 Cargo/Tauri 内部二进制名仍是 `cc-switch.exe`，正确做法是从 `cc-switch.exe` 复制，但在 portable zip 内重命名为 `CCSwitchMulti.exe`。
- 本次修复把 GitHub workflow 的 macOS tar/zip/dmg、DMG stage app/volname、Windows setup/MSI/portable、Linux AppImage/deb/rpm、release body 下载说明全部改成 `CCSwitchMulti-*` 外显命名；同时把 `src/i18n/locales/{en,ja,zh,zh-TW}.json` 顶层 `app.title` 改成 `CCSwitchMulti`，修掉窗口/document 标题的旧品牌残留。
- 验证通过：Prettier check 覆盖 workflow 和四个 locale、PowerShell `ConvertFrom-Json` 解析四个 locale、`pnpm typecheck`、`git diff --check`，以及 `rg 'CC-Switch|CC Switch' .github/workflows/release.yml` 无旧 release 外显命名残留。本机没有 `actionlint`，所以未做 workflow actionlint 验证。

## 2026-07-08 Codex Subagent Official Token Zero Repair

- 用户反馈 MultiRouter 的“今日子 Agent 会话流量”里 official/gpt 模型 token 没统计进去或显示 0。根因在 `src-tauri/src/services/usage_stats.rs::build_codex_subagent_usage_stats_from_history`：统计先聚合 `_codex_session` 数据库行，旧逻辑只有在某个子 Agent 完全没有 DB 用量行时才回退解析 rollout JSONL。official Codex OAuth/proxy 行可能已经有请求数但 token 字段全为 0，于是回退被阻断，真实 `token_count` 里的累计 token 没进入子 Agent 表。
- 修复边界：SQL 聚合阶段只先形成每个子 Agent 的会话桶，不立即写模型汇总；对每个子 Agent 都在时间范围允许时读取 rollout `token_count`，但只用 rollout 修正“DB 桶没有任何真实 token”的模型桶，避免 DeepSeek/Qwen 等已经同步成功的非零 token 被重复累加。模型汇总在修正完成后统一生成。
- 回归测试新增 `test_codex_subagent_usage_stats_repairs_zero_token_db_rows_from_rollout`：模拟 `gpt-5.5` 子 Agent 已有两条 `codex_session` 请求但 token 全 0，同时 rollout JSONL 有 `total_token_usage`，最终 agent/model 统计必须显示 1550 tokens 且 request_count 保持 2。
- 验证命令：`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo test --manifest-path src-tauri/Cargo.toml codex_subagent_usage_stats --lib`、`cargo test --manifest-path src-tauri/Cargo.toml test_sync_codex_subagent_uses_rollout_thread_id --lib`、`git diff --check`。
- 另一个相邻但未纳入本次 token 修复的发现：`gpt-5.3-codex-spark` 等 spark 变体如果 token 已有但 cost 为 0，可能是 `model_pricing` 种子缺少对应模型定价和历史成本回填，应该作为单独成本统计任务处理，避免和 token 修复混在一个提交里。

## 2026-07-06 CCSwitchMulti v3.16.4-15 GitHub Release Verification

- `v3.16.4-15` 已作为 `BigStrongSun/ccswitchmulti` 正式 release 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-15`。Release 为 `draft=false`、`prerelease=false`，发布时间为 `2026-07-06T14:24:42Z`。
- 版本提交为 `2b13e2d4731f89a0bf820d038d94611b741d2500`（`chore(release): bump CCSwitchMulti to v3.16.4-15`），annotated tag `v3.16.4-15` 指向该提交。`main` 和 tag 均已推送到 fork remote。
- GitHub Actions：main CI run `28796644943` 成功，Release run `28796647894` 成功；Release run 覆盖 `ubuntu-22.04`、`ubuntu-22.04-arm`、`windows-2022`、`windows-2022 arm64`、`macos-14`，`Publish GitHub Release` 和 `Assemble latest.json` 均成功。
- 发布资产共 19 个：Windows x64/ARM64 setup、portable 和签名，macOS dmg、tar.gz/signature、zip，Linux x64/ARM64 AppImage/signature/deb/rpm，以及 `latest.json`。远端 `latest.json` 已下载验证：`version=3.16.4-15`，包含 macOS、Windows x86_64/arm64、Linux x86_64/arm64 6 个 updater 平台条目。
- 本地固定交付目录 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti` 已由 post-commit release pipeline 刷新到 `Version: 3.16.4-15`、`Commit: 2b13e2d4731f89a0bf820d038d94611b741d2500`；Windows x64 NSIS setup、portable zip、raw exe 和 setup `.sig` 均已生成。
- 发布前本地验证覆盖：`CodexUsagePage` 单测、Codex 用量入口集成用例（当前环境需 `--testTimeout=20000`）、两个 temperature 缺省/显式 Rust 单测、`pnpm typecheck`、release note prettier check、`cargo fmt --check`、`git diff --check`。整文件 `tests/integration/App.test.tsx` 在默认 5 秒/10 秒阈值下仍会受既有 App/MSW 启动慢和测试隔离问题影响，不作为本轮 release 阻塞。

## 2026-07-06 CCSwitchMulti v3.16.4-15 Release Preparation

- 本轮用户要求“发一个新版本，发新的 release”。实际检查 `git log v3.16.4-14..HEAD` 后确认 `v3.16.4-14` 之后已有 5 个本地提交：新增 Codex 用量与重置额度工具页、补 temperature 默认边界测试、补 usage page 引导和主题颜色拆分、以及上一轮 release 验证 memory，因此不是空发。
- 版本号继续沿用 `v3.16.4-N` fork 递增规则，目标为 `3.16.4-15` / `v3.16.4-15`；同步更新 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.lock`，新增 `docs/release-notes/v3.16.4-15-zh.md`。
- 发布说明重点：Codex 专属用量与 reset credits 页面、banked reset credits/窗口状态展示、浅色/深色主题 token 拆分、temperature 缺省不补 0 的回归护栏。发布前需要跑页面相关 vitest、temperature Rust 单测、typecheck、prettier、cargo fmt 和 `git diff --check`。
- 注意边界：未跟踪 `output/release-v3.16.4-4-upload/`、`output/release-v3.16.4-5wizard/` 和 `scripts/logs/` 是旧输出/日志，不应加入 release commit；GitHub release 仍通过推送 annotated tag 触发 `.github/workflows/release.yml` 自动跨平台构建和上传资产。

## 2026-07-06 Codex Temperature Default Boundary

- 本轮排查的结论是不要在 CCSwitchMulti 收到 Codex `/v1/responses` 请求缺省 `temperature` 时全局补 `temperature=0`。外部检索和代码链路都显示：OpenAI Chat/Responses 的 `temperature` 是可选参数；GPT-5/o3 等 reasoning 模型公开反馈更多是“传了非默认 temperature 会 400”；Kimi/Roo-Code 反馈则是 `kimi-for-coding` 需要固定 `temperature=0.6`，默认补 0 反而会失败。因此“缺省补 0”不是通用修复。
- official Codex OAuth 路径必须继续不带 temperature：`src-tauri/src/proxy/providers/openai_compat.rs::normalize_codex_oauth_responses_request` 会删除 `temperature/top_p/max_output_tokens`，`src-tauri/src/proxy/providers/transform_responses.rs` 也在 `is_codex_oauth=true` 时删除这些字段；这是因为 ChatGPT Codex 反代后端不接受这些公开 OpenAI API sampling 字段。
- 第三方 Chat Completions 转换路径 `src-tauri/src/proxy/providers/transform_codex_chat.rs::responses_to_chat_completions_with_reasoning_text_only_and_cache` 的正确规则是：请求里已有 `temperature` 才透传；缺省时不自动补。若后续某个 provider/model 确认必须固定 temperature，应通过 provider/model 级 override 或显式配置注入特定值，而不是全局默认。
- 回归护栏：`responses_request_without_temperature_does_not_default_temperature` 固定缺省不补；`responses_request_with_temperature_preserves_explicit_temperature` 固定显式值原样透传。后续若要改 temperature 策略，必须同时考虑 OpenAI reasoning 模型拒绝该字段和 Kimi coding 固定 0.6 这两个反例。

## 2026-07-06 CCSwitchMulti v3.16.4-14 GitHub Release Verification

- `v3.16.4-14` 已作为 `BigStrongSun/ccswitchmulti` 正式 release 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-14`。Release 为 `draft=false`、`prerelease=false`，GitHub latest API 返回 `tag_name=v3.16.4-14`，发布时间为 `2026-07-06T03:04:19Z`。
- 版本提交为 `564829493183a577e3b5760126d83d9d860ef605`（`chore(release): bump CCSwitchMulti to v3.16.4-14`），annotated tag `v3.16.4-14` 指向该提交。`main` 和 tag 均已推送到 fork remote，`origin/upstream` 仍是原项目远端。
- GitHub Actions：Release run `28764056425` 成功，5 个平台构建、`Publish GitHub Release`、`Assemble latest.json` 全部成功；main CI run `28764054886` 成功，Frontend Checks 和 Backend Checks 均通过。
- 发布资产共 19 个：Windows x64/ARM64 setup、portable 和签名，macOS unsigned DMG、updater tarball/signature、zip，Linux x64/ARM64 AppImage/signature/deb/rpm，以及 `latest.json`。本次 macOS DMG 已上传，但因 Apple signing/cert 步骤跳过，仍按未签名版说明处理。
- `latest.json` 已下载验证：`version=3.16.4-14`，6 个 updater 平台的 URL 均指向 `v3.16.4-14`，签名字段均存在。Release 正文未命中 `ccswitch.io`、`farion1231/cc-switch`、`BigStrongSun/cc-switch` 等旧链接。
- 发布前本地验证覆盖：两组 Codex/MultiRouter/Provider vitest、`pnpm typecheck`、`cargo test --manifest-path src-tauri/Cargo.toml codex_responses_passthrough --lib -- --nocapture`、`cargo test --manifest-path src-tauri/Cargo.toml codex_reset --lib -- --nocapture`、`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、release note prettier check、`git diff --check`（仅 Cargo.lock CRLF 提示）。

## 2026-07-06 Codex Usage Reset Credits Page Entry

- 参考 `jordan-edai/codex-reset-watcher` 时，真正可复用的产品边界不是 macOS 菜单栏，而是跨平台的只读 Codex 用量面板：5 小时窗口、每周窗口、banked reset credits、到期紧迫度、部分明细失败提示和手动刷新。CCSwitchMulti 里不要把这类信息只藏在 provider footer。
- 前端已新增 Codex 专属工具页 `src/components/codex/CodexUsagePage.tsx`，数据源复用 `useSubscriptionQuota("codex", true, true, 5)`，仍读取本机 Codex 登录状态，不兑换 reset、不修改账号、不新增平台限定逻辑。
- 入口在主界面切到 Codex 后的顶部工具栏：`Codex 多模型路由` 图标旁边的 `Codex 用量与重置额度`（柱状图）按钮。`src/App.tsx` 的 `codexUsage` view 已加入 `VALID_VIEWS`，但切到非 Codex app 时会回退 providers，避免 Codex 专属工具跨 app 残留。
- 该页面必须保留显式引导和 CCSwitchMulti UI 匹配：顶部使用和 `CodexRouterWorkspacePage` 一致的渐变工作台 header，正文用蓝色引导卡说明“从 Codex 工具栏进入 -> 刷新当前登录 -> 按窗口和 reset 到期决策”。配色必须维护浅色/深色两套 surface token（当前集中在 `USAGE_PAGE_COLORS` / `READING_HINT_COLORS`），不要只用单套颜色叠 `dark:` 或退回成外部 dashboard 风格。
- 回归覆盖：`tests/integration/App.test.tsx` 固定 Codex 工具栏入口能打开 `CodexUsagePage`；`src/components/codex/CodexUsagePage.test.tsx` 固定成功数据、banked reset credits、reset 明细部分失败和无凭据状态不会空白。

## 2026-07-06 Codex 127.0.0.1:15721 502 VPN/Proxy Boundary

- 用户截图里的 `unexpected status 502 Bad Gateway: Unknown error, url: http://127.0.0.1:15721/v1/responses` 不能直接归因到 MultiRouter route miss。需要先区分两条边界：如果 `codex-router.log` 同时间没有 `route_resolved/request_prepared/upstream_send_error`，请求可能被 Codex Desktop 自身的系统代理/VPN/规则代理在到达 `127.0.0.1:15721` 前拦截；如果日志有 `upstream_send_error`，说明请求已进入 CC Switch，失败在 CC Switch 到真实上游的出站链路。
- 代码边界：Codex 到本地 `15721` 是入站 TCP，不读 CC Switch 的 `http_client`；CC Switch 出站分为 reqwest 和 hyper 路径。`src-tauri/src/proxy/http_client.rs` 未显式设置全局代理时会跟随当前进程可见的系统/环境代理线索，但只会对指向 CC Switch 自己端口的 loopback 代理做防自环跳过；`hyper_client.rs` 原始 TCP 路径不读代理环境变量。
- 产品侧修复点：`diagnose_codex_multirouter` 新增「出站代理 / VPN 环境」检查，只读展示 CC Switch 显式全局代理、当前进程 `HTTP_PROXY/HTTPS_PROXY/ALL_PROXY/NO_PROXY`、Windows WinINET 用户代理和 WinHTTP 摘要；FAQ 增加 `502 Bad Gateway: 127.0.0.1:15721/v1/responses` 排障流程，提示用户通过 router 日志区分“请求未到本地代理”和“进入本地代理后上游出站失败”，并在规则代理失败时尝试 localhost 绕过、全局代理或 TUN 模式。
- 后续遇到同类反馈时，先让用户运行 Codex MultiRouter Debug 检查并导出 `~/.cc-switch/logs/codex-router.log` 同时间窗口；不要只根据 UI 里的 `Unknown error` 断定是 provider 配置、OAuth 或模型转换问题。

## 2026-07-06 GitHub Page Copy Cleanup Before Fork Detach

- 按用户要求降低仓库首页对官网和原项目的显式导流：GitHub repository `homepage` 已清空，description 改为 `CCSwitchMulti: Codex 多模型路由，把 OpenAI 订阅与 DeepSeek/Qwen/本地/第三方 OpenAI-compatible 模型合并到 Codex。`，topics 移除了 `cc-switch`。
- 四个 README 顶部都改为 `CCSwitchMulti` / Codex MultiRouter 自身定位，release/download badge 指向 `BigStrongSun/ccswitchmulti`，删除 `ccswitch.io` 官网行和 `farion1231/cc-switch` 的 Trendshift/star-history 顶部徽章；底部 star-history 仍保留但已改为当前仓库 `BigStrongSun/ccswitchmulti`。
- 主 README 分支说明去掉原项目链接，改为 CCSwitchMulti 自身能力说明；issue templates 的 FAQ、existing issues、Security Advisories、Discussions 链接全部改到 `BigStrongSun/ccswitchmulti`；release workflow 自动正文不再追加官网链接。
- 边界：GitHub API 仍显示 `parent=farion1231/cc-switch`，这是 fork network 元数据，不能通过 README/metadata 文案清理去掉；只有 GitHub Support detach fork network 或重建独立仓库后才会消失。

## 2026-07-06 GitHub Discoverability Low-Risk Optimization

- 在不改变 fork network 的前提下，已把 `BigStrongSun/ccswitchmulti` 的 GitHub description 改成同时包含 `CCSwitchMulti`、`Codex`、`OpenAI-compatible`、`DeepSeek`、`Qwen`、`CC Switch` 等中英文关键词；homepage 保持 `https://ccswitch.io`。
- 已补齐仓库 topics：`ccswitchmulti`、`cc-switch`、`codex`、`codex-multirouter`、`multi-model-router`、`openai`、`openai-compatible`、`deepseek`、`qwen`、`local-llm`、`desktop-app`、`tauri`、`provider-management`、`local-proxy`、`ai-tools`。
- README 顶部 version/download/release badge、分支说明和使用注意里的旧 `BigStrongSun/cc-switch` 链接已改为 `BigStrongSun/ccswitchmulti`；`docs/codex-spawn-agent-model-candidates.md` 的 tracking issue 也改到当前 slug。`rg` 验证目标文件中旧 slug 不再出现。
- 这些优化有助于外部搜索、GitHub topic 浏览和用户识别当前仓库，但不会改变 GitHub repository/code search 默认隐藏 fork 的规则；默认搜索可见性仍需要脱离 fork network 或要求用户加 `fork:true`。

## 2026-07-06 GitHub Repository Search Fork Visibility

- `BigStrongSun/ccswitchmulti` 直链可访问且 GitHub API 显示 `private=false`、`visibility=public`、`archived=false`、`disabled=false`，默认分支是 `main`，父仓库是 `farion1231/cc-switch`。因此“搜不到”不是私有、归档、禁用或远端地址错误。
- 根因边界是 GitHub 普通 repository search 默认不显示 fork。实测 `gh api 'search/repositories?q=ccswitchmulti&per_page=10'` 和 `q=ccswitchmulti user:BigStrongSun` 都返回 `total_count=0`；加 `fork:true` 或 `fork:only` 后返回 6 个 fork，第一项就是 `BigStrongSun/ccswitchmulti`。
- 代码搜索同样受 fork 过滤影响：`gh api 'search/code?q=repo:BigStrongSun/ccswitchmulti CCSwitchMulti'` 返回 0；加 `fork:true` 后能搜到仓库内 56 个结果。GitHub connector 的安装仓库查询显示 `is_code_search_indexed=true`，说明不是代码索引缺失。
- 外部搜索并非完全未收录：matrix-websearch 能搜到 GitHub 仓库页、releases 页和相关文章；GitHub 页面没有发现 `X-Robots-Tag` 或 `<meta name="robots">` 禁止索引信号。用户体感上的主要问题是 GitHub 站内默认搜索不带 fork 过滤。
- 改善建议：如果希望普通用户不加 `fork:true` 也能在 GitHub repository search 更稳定地找到，需要考虑脱离 fork network 变成独立仓库；但 GitHub 官方 `Leave fork network` 要求 public、少于 1GB、且没有 child forks，当前 API 显示本仓库已有 6 个 forks，直接脱离可能被限制。手动重建独立仓库会丢失 issues、PR、stars、watchers、child forks 等仓库元数据，只保留 git commit metadata。
- 低风险改善是给仓库加 topics、确保 description/README 使用 `ccswitchmulti`，并把 README 中旧 `BigStrongSun/cc-switch` badge/release 链接更新为 `BigStrongSun/ccswitchmulti`，但这些不会改变 fork 默认过滤规则。

## 2026-07-05 Codex Provider Catalog vs Menu Mapping Boundary

- 用户反馈新版 Codex provider 配置里，只有打开“需要本地路由映射”才展开模型列表；但 Responses 原生 provider 也应该能获取 `/models`、保存模型目录和上下文窗口。根因是前端把 `takeoverEnabled` 同时当成“目录/上下文编辑开关”和“Codex /model 菜单映射开关”，后端旧语义又把 `modelCatalog` 的存在直接解释成要写 `model_catalog_json`。
- 新不变量：`settingsConfig.modelCatalog` 是 cc-switch 的模型目录、上下文窗口、MultiRouter/子 Agent 候选元数据 SSOT；`meta.codexLocalModelMapping` 只控制单 provider 是否把该目录投射到 Codex `/model` 菜单和本地模型名映射。关闭菜单映射时仍要保存 catalog；开启 MultiRouter routes 时仍强制投射聚合 catalog。
- UI 文案边界：把“需要本地路由映射”改成“在 Codex /model 菜单中显示”，把顶部“本地模型路由”改成“Codex 多模型路由”，明确前者是菜单显示/单 provider 模型名映射，后者是一个 provider 内按 `body.model` 分流到多上游。`获取模型列表` 和 `测试 Chat / Responses` 是配置主操作，不能被菜单映射开关隐藏。
- 兼容边界：旧 provider 没有 `codexLocalModelMapping` 字段但已有 `modelCatalog` 时继续沿用旧行为投射，避免老用户升级后 `/model` 菜单消失；新 provider 显式保存 `false` 后，live 写入前会移除投射用的 `modelCatalog`，但 DB 里的目录元数据保持不变。

## 2026-07-05 MultiRouter Wizard Nested Provider Dialog Layering

- 用户反馈 MultiRouter 配置向导里保存/新增 provider 时弹窗像没到最前或卡死。根因不是 provider 保存 API 卡住，而是向导内再打开新增 provider 后，新增面板内部的二级弹层仍按默认层级 portal 到 `document.body`：`CodexFormFields` 的混合协议拆分/route 编辑确认、`UniversalProviderFormModal`、`ConfirmDialog` 等可能被 `FullScreenPanel z-[140]` 或向导壳遮住。
- 修复边界：不要只把某一个弹窗硬编码到更高 z-index。`FullScreenPanel` 现在提供弹层上下文，面板内未显式指定层级的 `DialogContent` 默认使用 `top`；`ConfirmDialog` 默认遵循上下文再退回 `alert`；嵌套 `FullScreenPanel` 自动使用下一层，向导新增 provider 面板 `z-[140]` 内的子面板提升为 `z-[160]`。
- 同轮修复了 `FullScreenPanel` 对 `document.body.style.overflow` 的竞争：多个全屏面板嵌套时用引用计数锁滚动，避免子面板关闭时提前解除父面板仍需要的滚动锁。
- 回归测试：`tests/components/FullScreenPanelLayering.test.tsx` 固定全屏面板内部普通 Dialog、ConfirmDialog 均在 `z-[200]`，并固定 `z-[140]` 父面板内的子 FullScreenPanel 为 `z-[160]`。

## 2026-07-04 Codex MultiRouter Wizard OAuth Guidance

- 用户指出 MultiRouter 设置向导少了关键步骤：没有引导用户配置官方 Codex OAuth。根因是向导按普通 `API Key + Base URL + /models` 范式处理所有模型源，虽然后端能把 official target 物化为 `codex_oauth`，但前端配置步骤只显示“已有模型目录/缺 Base URL”，没有展示 ChatGPT 登录入口。
- 修复边界：`isWizardCodexOAuthSource()` 统一识别 `category=official`、`providerType=codex_oauth`、`authBinding/source=managed_codex_oauth`、`auth_mode=chatgpt`、ChatGPT backend base URL 和 OpenAI Official 名称。OAuth 源不参与普通 `/models` 获取，也不参与 API Key 的 Chat/Responses 双协议探测；向导使用官方内置 fallback catalog，并在 providerConfig 步骤嵌入 `CodexOAuthSection`。
- 保存语义：向导生成 official/OAuth route 时显式写 `upstream.auth.source = managed_codex_oauth` 和 `authProvider = codex_oauth`，已有绑定账号则保留 `accountId`。这不是替代后端兜底，而是让保存后的 MultiRouter plan 自描述，便于工作台和日志排查。
- 回归覆盖：`tests/lib/codexMultiRouterWizard.test.ts` 固定 official provider 即使被旧 `base_url/apiKey` 污染也不作为模型抓取源，且 route auth 写 managed OAuth；`tests/components/CodexMultiRouterWizard.test.tsx` 固定配置步骤必须展示 ChatGPT OAuth 引导和登录区，不再给 official provider 显示普通 API 格式下拉。

## 2026-07-04 Codex Local Routing Active Notice

- App 顶层用 `useCodexLocalRoutingNotice(Boolean(isProxyRunning && takeoverStatus?.codex))` 监听 Codex 本地路由真实启用状态；触发条件是 CCSwitchMulti 本地代理正在运行且 Codex 接管已开启，而不是当前页面是否停留在 Codex。
- 提示弹窗文案为：`您正在使用本地路由功能，将由 ccsm 接管所有 codex 流量，所以不要在使用 codex 时关闭本软件。` Hook 只监听 `false -> true` 边沿；用户点“我知道了”后不因状态轮询重复弹，只有本地路由先关闭再重新开启时再次提醒。
- 回归测试：`tests/hooks/useCodexLocalRoutingNotice.test.tsx` 覆盖首次开启弹出、确认后保持开启不重复弹、关闭后重新开启再次弹。

## 2026-07-04 Codex MultiRouter Provider Catalog Selection Sync

- 用户反馈在普通 Codex provider 里删减保留模型后，进入 MultiRouter 配置或设置向导时远端 catalog 又全量出现。根因不是 provider 保存后的 `syncCodexMultiRouterPlanWithProviders()` 没跑，而是 MultiRouter 工作台 `providerWithFetchedModelCatalog()` 和向导 `mergeFetchedModelsIntoWizardProvider()` 在自动刷新 `/models` 时把远端全量模型追加回 `modelCatalog.models`，把用户保留列表反向污染成发现列表。
- 状态源边界：普通 provider 编辑页的“获取模型列表”是显式编辑/发现入口，可以让用户重新看到全量模型；MultiRouter 工作台和设置向导的自动刷新属于路由构建链路，`modelCatalog.models` 必须按“用户保留/暴露列表”解释。已有目录只更新匹配模型的 context window 等元数据，空目录才用远端 `/models` 初始化。
- 子 Agent 候选必须跟随同一保留列表：向导刷新 provider 时会剪掉不在当前 `modelCatalog.models` 内的 `modelCatalog.spawnAgentModels`，工作台刷新继续通过 `normalizeCodexSpawnAgentModels()` 剪枝；已保存 MultiRouter plan 仍由 `syncCodexMultiRouterPlanWithProviders()` 重建 route/catalog/spawnAgentModels。
- 回归覆盖：`mergeFetchedModelsIntoWizardProvider(..., { preserveExistingSelection: true })` 保留 alias/upstream 并只更新已保留模型；向导刷新不会把 extra upstream model 写回 provider；工作台 routes 自动刷新不会恢复 provider 已删除模型，并会把 stale plan route/catalog/spawnAgentModels 同步剪掉；AgentPlan 在线获取能力保留在“空目录初始化”场景。

## 2026-07-04 MiniMax Native Responses Function Arguments Strictness

- 用户截图报错：`CC Switch local proxy failed while handling Codex endpoint /responses. Provider: MiniMax; model: MiniMax-M3; upstream_status: HTTP 400; cause: invalid params, invalid function arguments json string, tool_call_id: call_function_... (2013)`。这不是本地日志缺失问题，也不是 MiniMax preset 选错；错误来自 MiniMax native Responses 上游重新解析历史 `function_call.arguments` 时发现 JSON 字符串非法。
- 外部同类案例和本地既有代码都指向同一类根因：严格上游（MiniMax 已确认）会拒绝空字符串、被截断的 `{` / `...[truncated]` 等非法 JSON arguments；宽松上游可能静默接受。此前 `json_canonical::canonicalize_tool_arguments` 已修复 Responses->Chat 转换路径，但第三方 native Responses passthrough 只提升 system/developer 控制消息，没有清理 `type=function_call` 的 `arguments`。
- 修复边界：`openai_compat.rs::normalize_codex_responses_passthrough_request` 现在同时调用 `normalize_codex_responses_function_call_arguments`，仅对 `input[*].type == "function_call"` 规整 `arguments`：缺失/空字符串转 `{}`，合法 JSON 做 canonical 输出，非法非空片段包进 `{"raw_arguments":"..."}`。不改 route、不把 MiniMax 改成 Chat、不全局删除 Responses 字段。
- 回归测试：`codex_responses_passthrough_normalizes_function_call_arguments` 覆盖 MiniMax-M3 native Responses 历史中空 arguments 和 `{` 片段；同时复跑 `codex_responses_passthrough`、`codex_oauth_responses_normalizer` 和 `responses_request_to_chat_sanitizes*`，确认第三方 passthrough、official OAuth、Responses->Chat 三条链路都未回退。

## 2026-07-03 Xiaomi MiMo Codex Native Responses Preset Boundary

- GitHub issue `BigStrongSun/ccswitchmulti#8` 反馈 `v3.16.4-11` 下非 Token Plan 小米 MiMo `mimo-v2.5-pro` 在 Codex 编码任务中途停下、需要手动“继续”。这不是 MultiRouter GPT 错路由或 official OAuth cleanup 问题；当前 MiMo Codex preset 是 native Responses 直连，核心差异是 preset 只用了通用 `generateThirdPartyConfig`，漏掉了小米官方 Codex 示例要求的 Codex 层字段。
- 小米官方 Codex 配置示例明确要求 `model_supports_reasoning_summaries = true`、`model_reasoning_summary = "none"`、`model_context_window = 1048576`、`web_search = "disabled"`、`wire_api = "responses"`。缺少 `model_supports_reasoning_summaries` 时，原版 Codex 的 `build_reasoning` 会把 `model_reasoning_effort = "high"` 当成无效，不发送 `reasoning` 参数；缺少 `model_context_window` 时 TurnContext/token usage/auto-compact 只能走模型 catalog 或兜底上下文，容易和 MiMo 1M 窗口不一致。
- 修复边界：不要为了补 MiMo 元数据给 native Responses preset 加 `modelCatalog`，因为 ProviderForm 仍用 `modelCatalog.models.length > 0` 推断“本地路由/接管”初始开关。MiMo 的默认单模型直连应靠 TOML 顶层字段修复，仍保持 `apiFormat = "openai_responses"` 且 `modelCatalog` 为空，避免把用户无代理直连路径改成本地接管。
- 回归测试落点：`tests/config/codexChatProviderPresets.test.ts` 固定两个 MiMo Codex preset 同时包含官方 reasoning/context/web_search 字段、使用各自 base URL、模型为 `mimo-v2.5-pro`、`wire_api=responses`，并继续断言不带 `modelCatalog`。

## 2026-07-03 CCSwitchMulti v3.16.4-12 GitHub Release Verification

- `v3.16.4-12` 已作为 BigStrongSun/ccswitchmulti 的正式 release 发布并通过 GitHub Actions release run `28647799609`，head sha 为 `70bf31ed19416c723ef58d1c4a92ddda29023fe2`。五个平台 build、Publish GitHub Release、Assemble `latest.json` 全部成功。
- Annotated tag `v3.16.4-12` 的 tag object 为 `b0e83bbcfacf31efa089d3e4a06e35e9799933c2`，解引用到 release bump 提交 `70bf31ed19416c723ef58d1c4a92ddda29023fe2`。`fork/main` 也已推到同一提交；`origin/upstream` 仍是原版 `farion1231/cc-switch`，本次未向原版远端发布。
- Release `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-12` 为 `draft=false`、`prerelease=false`，GitHub latest API 已返回 `tag_name=v3.16.4-12`，release name 为 `CCSwitchMulti v3.16.4-12`，正文来自 `docs/release-notes/v3.16.4-12-zh.md` 并包含本轮 Codex 跨 provider 路由与 official OAuth reasoning 历史兼容修复说明。
- 发布资产共 19 个：macOS unsigned DMG、macOS updater tarball/signature、macOS zip、Windows x64/ARM64 setup/portable/signature、Linux x64/ARM64 AppImage/signature/deb/rpm，以及 `latest.json`。
- `latest.json` 已下载并解析成功：`version=3.16.4-12`，包含 6 个 updater 平台；`darwin-aarch64` 和 `darwin-x86_64` 都指向 `CC-Switch-v3.16.4-12-macOS.tar.gz`，`windows-aarch64` 指向 `CC-Switch-v3.16.4-12-Windows-arm64-Setup.exe`，所有平台 URL 都指向 `v3.16.4-12` 且签名字段存在。
- 发布前本地验证覆盖：`rg` 确认四个版本面无 `3.16.4-11` 残留；`pnpm exec prettier --check package.json src-tauri/tauri.conf.json docs/release-notes/v3.16.4-12-zh.md`；`cargo fmt --manifest-path src-tauri\Cargo.toml --check`；`cargo test --manifest-path src-tauri\Cargo.toml codex_oauth_responses_normalizer --lib -- --nocapture`；`cargo test --manifest-path src-tauri\Cargo.toml codex_responses_passthrough --lib -- --nocapture`；`cargo test --manifest-path src-tauri\Cargo.toml proxy::providers::codex::tests --lib -- --nocapture`；`cargo test --manifest-path src-tauri\Cargo.toml proxy::providers::openai_compat::tests --lib -- --nocapture`；`git diff --check`。

## 2026-07-03 Codex Cross-Provider Model Switch Type Boundary

- 原版 Codex 的 `/model` 切换不重写历史 item：TUI 通过 `AppEvent::UpdateModel` / `Op::OverrideTurnContext` 更新当前 thread settings，core 侧把 `model/effort` 合进 `SessionConfiguration.collaboration_mode`，下一轮再用新的 `TurnContext.model_info` 构造请求；历史仍来自 `clone_history().for_prompt(...)` 并作为 `Prompt.input` 进入 `ResponsesApiRequest.input`。
- 因此 CCSwitchMulti 的正确边界不是在 route runtime 猜“上一轮来自哪个 provider”，也不是把历史全局改成某个上游的 schema；应把 Codex 历史视为 canonical Responses-like item，进入 provider 之前按目标 provider 的 wire schema 做 request-local normalization。
- Chat 桥接路径专属字段如 `reasoning_content` 只属于 Responses->Chat / Chat->Responses 适配层和 `CodexChatHistoryStore` 缓存，用来给 DeepSeek/Kimi/MiMo 等 Chat 上游恢复 assistant tool-call reasoning；它不应作为 native Responses 或 official OAuth 的通用字段。官方 Codex 协议的 FunctionCall/ToolSearchCall 类型本身不声明 `reasoning_content`，Codex 反序列化后也不会把该字段自然持久化为正式历史字段。
- Official ChatGPT Codex OAuth 私有 `/backend-api/codex/responses` 比公开 Responses 更严格：出站前必须提升 system/developer input 到顶层 `instructions`，保留 `message.content`，reasoning item 只保留 `summary/encrypted_content` 并把缺失 summary 的 raw content 转为 `summary_text` 后移除 raw `content`，tool/call output 上的冗余 `content` 也要移除，同时确保 `include` 带 `reasoning.encrypted_content`。
- 第三方 native Responses 直透目前只做 system/developer 控制消息提升；不要无证据套用 official OAuth 私有 cleanup。若某个第三方 Responses 也拒绝 `reasoning.content` 或工具 output `content`，应按 provider/API-format 增加局部 compatibility normalizer 和回归测试，而不是全局删字段影响其它公开 Responses 兼容实现。
- 这次 raw `reasoning.content` 问题的引入链路是：`15e712e7` 打开第三方 native Responses 直连后，同一 session 更容易产生/保留 Responses-shaped 历史；`77781164` 的 official OAuth cleanup 只处理 tool/output `content`，当时错误地允许 `reasoning.content` 保留；`6524fe2d` 补了切模型 control-message 提升但没有改变 reasoning 边界。最终修复应落在 official OAuth 出站 normalizer，而不是改变原版 `/model` 语义。

## 2026-07-03 Codex Official OAuth Reasoning Content Boundary

- 更新 2026-07-01 的 Responses input 清理边界：official managed Codex OAuth 不能再原样保留 `type=reasoning` item 上的 raw `content`。真实上游错误 `Invalid input[n].content: array too long. Expected an array with maximum length 0` 同样会发生在 reasoning item；这说明 ChatGPT Codex 私有 `/responses` input schema 不接受 reasoning.content。
- 修复边界不是丢弃 reasoning 状态：`openai_compat.rs::normalize_codex_oauth_responses_request` 仍保留普通 `message.content`，并保留 reasoning item 的 `encrypted_content` 与已有 `summary`；只有没有 summary 的旧会话/第三方转换形态，才把 raw `content` 中可读文本提升为 `summary: [{type:"summary_text", text:...}]` 后移除 raw `content`。公开 OpenAI Responses、第三方 Responses、Responses->Chat 转换路径不调用这条 official OAuth 专属清理。
- 路由错分流的另一个根因是旧 DB/接管备份可能把 `codex-official` 目标 provider 污染成带第三方 `base_url`/`apiKey`。MultiRouter materialize 时官方身份应优先于污染的普通 API 字段：`targetProviderId=codex-official` 或 official/category/OAuth auth 命中时物化为 `meta.provider_type=codex_oauth`，并只在 request-local effective provider 上移除 `base_url/baseURL/baseUrl/apiKey/api_key`，持久 provider 不被重写。
- 回归测试应覆盖两类场景：reasoning 带 `summary+encrypted_content+content` 时移除 content 且保留 summary/encrypted_content；reasoning 只有 raw content 时提升为 summary_text；污染的 `codex-official` target provider 仍提取 `https://chatgpt.com/backend-api/codex` 与 `AuthStrategy::CodexOAuth`，且不走 Responses->Chat。

## 2026-07-03 GitHub Release Body Must Use CCSwitchMulti Notes

- Release tag 本身应继续使用推送的 fork tag（例如 `v3.16.4-11`），不要从 upstream/origin 取原版 tag 或 release 文案。`release.yml` 的 `tag_name: ${{ github.ref_name }}` 是正确边界。
- 旧 GitHub build release 的问题不是 tag 错，而是 Release name/body 仍是泛模板 `CC Switch <tag>` 和下载说明，缺少 CCSwitchMulti 的功能更新和 bug 修复内容。
- 修复边界：`Publish GitHub Release` job 必须 checkout 仓库，生成 `release-body.md`，优先读取 `docs/release-notes/${tag}-zh.md`，再追加下载资产说明；`softprops/action-gh-release` 使用 `name: CCSwitchMulti ${tag}` 和 `body_path: release-body.md`。
- `v3.16.4-11` 当前 GitHub release 已手动更新为 `CCSwitchMulti v3.16.4-11`，正文包含功能更新、Bug 修复、继承的核心产品修复、验证和下载说明。后续新 tag 会由 workflow 自动使用同样规则。

## 2026-07-03 CCSwitchMulti v3.16.4-11 GitHub Release Verification

- `v3.16.4-11` 已作为 BigStrongSun/ccswitchmulti 的正式 release 发布并通过 GitHub Actions release run `28616009981`，head sha 为 `8e93cd6df6b7737c3420a5b6861de41992449ca8`。五个平台 build、Publish GitHub Release、Assemble `latest.json` 全部成功。
- Release `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-11` 为 `draft=false`、`prerelease=false`，GitHub latest API 已返回 `tag_name=v3.16.4-11`。资产共 19 个，包含 unsigned `CC-Switch-v3.16.4-11-macOS.dmg`、macOS updater `macOS.tar.gz`/`.sig`、macOS zip、Windows x64/ARM64 setup/portable/signature、Linux x64/ARM64 AppImage/deb/rpm/signature、以及 `latest.json`。
- macOS job 日志确认 Apple secrets 仍为空，`APPLE_SIGNING_IDENTITY` 为空，workflow 明确走 unsigned fallback：复制 `src-tauri/target/universal-apple-darwin/release/bundle/dmg/CCSwitchMulti_3.16.4-11_universal.dmg` 到 `release-assets/CC-Switch-v3.16.4-11-macOS.dmg`，跳过 notarization 和 signed verification。这符合“允许接受未签名版本，后续再补 mac 签名”的发布边界。
- `latest.json` 已下载并解析成功：`version=3.16.4-11`，包含 6 个 updater 平台；`darwin-aarch64` 和 `darwin-x86_64` 都指向同一个 universal `CC-Switch-v3.16.4-11-macOS.tar.gz`，`windows-aarch64` 指向 `CC-Switch-v3.16.4-11-Windows-arm64-Setup.exe`。

## 2026-07-03 GitHub Release latest.json Retry Boundary

- `v3.16.4-10` 验证了 unsigned macOS DMG fallback 是有效的：Release 资产已包含 `CC-Switch-v3.16.4-10-macOS.dmg`、`macOS.tar.gz`、`.sig` 和 `macOS.zip`，说明 macOS build 和 `Prepare macOS Assets` 修复没有问题。
- 同一 run `28613873120` 最终失败在 `Assemble latest.json` 的 `Download all release assets`：`gh release download v3.16.4-10` 调 GitHub asset API 时返回 HTTP 503，导致 `latest.json` 没生成。这个失败点发生在资产上传之后，所以 Release 看起来可下载但缺 updater 元数据。
- 修复边界：`assemble-latest-json` 不能单次依赖 GitHub release asset API；下载全部 release 资产和上传 `latest.json` 都要有限重试，失败时保留明确错误。不要把这种 503 误判成 macOS DMG fallback 失败。
- 后续发布应推进新 tag 覆盖半成功 release，而不是只手工给旧 release 补 `latest.json`；否则 workflow 本身仍会在下一次 GitHub API 抖动时复发。

## 2026-07-03 macOS Unsigned DMG Release Fallback

- `v3.16.4-9` 的 GitHub macOS job 已证明 Tauri 会在 `pnpm tauri build --target universal-apple-darwin` 阶段生成 `src-tauri/target/universal-apple-darwin/release/bundle/dmg/CCSwitchMulti_*_universal.dmg`，即使仓库缺少 Apple 签名和公证 secrets。
- 真实缺口在 `Prepare macOS Assets`：旧逻辑在 `APPLE_SIGNING_IDENTITY` 为空时只上传 updater tarball 和 app zip，然后 `exit 0`，导致 Release 成功但没有 macOS DMG。
- 修复边界：无 Apple 签名时也必须把 Tauri 生成的 unsigned DMG 复制为 `CC-Switch-${tag}-macOS.dmg` 并上传；如果无签名且找不到 `.dmg`，macOS job 应失败而不是静默缺资产。Apple secrets 齐全时仍走 `create-dmg` 的签名/公证路径。
- Release 文案必须明确：缺 Apple 签名配置时 macOS DMG 是未签名版本；补齐 Apple Developer ID 证书和 notarization secrets 后，同一 workflow 会自动发布签名并公证的 DMG。

## 2026-07-03 CCSwitchMulti v3.16.4-9 GitHub Release Verification

- `v3.16.4-9` 已推到 `BigStrongSun/ccswitchmulti` 并通过 GitHub Actions release run `28610511658`，head sha 为 `0e8b25cdd0cbfe8e2bff054b46850ce1c5215c0e`。该 run 覆盖 Linux x64、Linux ARM64、Windows x64、Windows ARM64、macOS universal、Publish GitHub Release 和 Assemble `latest.json`，全部成功。
- 这次验证了 Windows ARM64 从 WiX MSI 切到 NSIS 的修复是有效的：release 资产包含 `CC-Switch-v3.16.4-9-Windows-arm64-Setup.exe`、`.sig` 和 `Windows-arm64-Portable.zip`，`latest.json` 的 `windows-aarch64.url` 也指向 `Windows-arm64-Setup.exe`。
- macOS 会自动 build：workflow 的 `macos-14` job 会构建 `aarch64-apple-darwin` 与 `x86_64-apple-darwin` target，合成 universal `codex-history-repairer` sidecar，并执行 `pnpm tauri build --target universal-apple-darwin`。本次产物包含 `CC-Switch-v3.16.4-9-macOS.tar.gz`、`.sig` 和 `CC-Switch-v3.16.4-9-macOS.zip`，`latest.json` 同时给 `darwin-aarch64` / `darwin-x86_64` 指向同一个 universal updater tarball。
- 本次没有发布 macOS `.dmg`，不是 macOS 自动 build 失败，而是 GitHub Actions 日志显示 `APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、`APPLE_ID`、`APPLE_PASSWORD`、`APPLE_TEAM_ID` 都为空，`APPLE_SIGNING_IDENTITY` 因此缺失。workflow 只在 Apple 签名/公证凭据齐全时生成并上传签名 DMG；缺凭据时只发布 updater tarball 和 app zip。后续如果要有正式 DMG，需要先补齐 Apple Developer ID 证书与 notarization secrets，或者明确决定上传未签名 Tauri 生成的 DMG。
- 发布后校验：`gh api repos/BigStrongSun/ccswitchmulti/releases/latest` 返回 `tag_name=v3.16.4-9`、`draft=false`、`prerelease=false`；release 资产共 18 个，包含 `latest.json`，但不包含 `.dmg`。

## 2026-07-03 GitHub Release Windows ARM64 NSIS Fallback

- `v3.16.4-8` tag 验证了上一轮 CI/Release 修复的大部分链路：main push CI 成功；Release 的 Linux x64、Linux ARM64、Windows x64 均成功；Windows ARM64 已成功交叉编译主程序和 `codex-history-repairer.exe` sidecar。
- 新失败点仍在 Windows ARM64 安装包阶段：x64 `windows-2022` runner 上执行 `pnpm tauri build --target aarch64-pc-windows-msvc --bundles msi` 时，主程序已构建到 `src-tauri/target/aarch64-pc-windows-msvc/release/cc-switch.exe`，但 WiX v3 `light.exe` 在 `Running light to produce ..._arm64_en-US.msi` 后失败，日志只有 `failed to run ...\WixTools314\light.exe`。这说明把 ARM64 从 `windows-11-arm` 挪到 x64 runner 只能解决 runner 环境启动问题，不能保证 WiX3 ARM64 MSI 打包可靠。
- 修复边界：Windows ARM64 release 不再强制产 MSI，改成 x64 runner 交叉编译 `aarch64-pc-windows-msvc` 后用 NSIS 生成 `CC-Switch-v*-Windows-arm64-Setup.exe`，同时继续产出 `Windows-arm64-Portable.zip`。MSI 只作为兼容旧发布的可选资产，缺失不再阻塞 Release。
- 相关 workflow 不变量：`assemble-latest-json` 已支持 `*-Windows-arm64-Setup.exe` 映射到 `windows-aarch64`；`Prepare Windows Assets` 必须先收集 NSIS 并要求签名，再把 MSI 作为可选附加资产；GitHub Release 下载文案必须写 ARM64 `Setup.exe` 而不是 `.msi`。

## 2026-07-03 GitHub Flow CI/Release Failure Root Fix

- `v3.16.4-7` 推到 `BigStrongSun/ccswitchmulti` 后 GitHub Actions 有两类真实失败：CI 的 `Backend Checks` 卡在 Rust Clippy，Release 的 Windows 打包卡在 Windows 资产构建。不要把它归因为 GitHub 本身抽风，也不要只重跑 workflow。
- CI 根因之一是火山 AgentPlan 模型列表支持把 `fetch_models_for_config` / `model_fetch::fetch_models` 扩成 8 个并列参数，触发 `clippy::too_many_arguments`。修复边界是把 Tauri 命令入参收敛成 `FetchModelsForConfigRequest`，后端服务收敛成 `FetchModelsRequest` / `VolcengineModelListRequest`，前端 `invoke` 改为 `{ request: ... }`。
- CI 继续暴露的 backend test 根因是 Codex provider 配置合并边界不清：`model_context_window` 应视为用户界面/上下文显示偏好，provider 未声明时不能从 live config 删除；同名 custom provider 表如果 live 里仍带本地代理 `base_url` 或 `PROXY_MANAGED`，恢复 provider 备份时必须按 takeover 残留整表替换，否则本地代理字段会继续劫持后续请求。
- Release Windows x64 根因是 Tauri bundler 需要 `codex-history-repairer.exe` sidecar，但 workflow 只构建主程序，NSIS 阶段报 `codex-history-repairer.exe` 不存在。修复是在 Windows Tauri build 前显式按架构构建并校验 history repair sidecar。
- Release Windows ARM64 根因是 `windows-11-arm` runner 能编译主程序，但 WiX v3 `light.exe` 在该 ARM runner 环境无法可靠启动；正式 workflow 应在 `windows-2022` x64 runner 上交叉构建 `aarch64-pc-windows-msvc`，同时 pnpm setup/cache 条件要看 `runner.arch`，不要看目标 `matrix.arch`。
- `release.yml` 的正式发布语义必须是 `prerelease: false`；否则 GitHub latest 不会按正式 release 晋升，即使 tag 和资产都存在。
- 本轮本地验证覆盖：`cargo clippy --manifest-path src-tauri\Cargo.toml -- -D warnings`、`cargo test --manifest-path src-tauri\Cargo.toml`、`pnpm typecheck`、`pnpm test:unit`、`pnpm exec prettier --check src/lib/api/model-fetch.ts .github/workflows/release.yml`。`actionlint` 本机未安装，后续若继续改 workflow，优先补装或在 CI 侧验证 workflow 语法。

## 2026-07-02 CCSwitchMulti v3.16.4-7 Formal Release

- `v3.16.4-7` 是 `v3.16.4-6` 后的 MultiRouter 路由热修复正式发布，核心变更是修复第三方 GPT alias 只出现在聚合 `modelCatalog`、没有回到第三方 route `match.models/modelMap` 时被官方 `gpt` 前缀 route 抢走的问题；版本提交为 `755b69e4ee0b5a91461558e4b7a8d8753b38bc5e`（`chore(release): bump CCSwitchMulti to v3.16.4-7`）。
- 本地正式输出目录：`C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.4-7`。验证：`latest.json` 版本为 `3.16.4-7` 且下载 URL 指向 `https://github.com/BigStrongSun/ccswitchmulti/releases/download/v3.16.4-7/CCSwitchMulti_3.16.4-7_x64-setup.exe`；raw exe `FileVersion/ProductVersion=3.16.4-7`。
- `v3.16.4-7` 已作为 BigStrongSun/ccswitchmulti 的 GitHub 正式 release 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-7`。Release 为非 draft、`prerelease=false`，latest API 已返回 `tag_name=v3.16.4-7`，annotated tag 对象 `8d0890abefef939681724e350aad5e103d8d5a37` 解引用到 `755b69e4ee0b5a91461558e4b7a8d8753b38bc5e`。
- 本地/远端主资产 SHA256 对齐：setup `FC1D50037CB4FFC2C6BD008EEA6F72222DB33893A744128B50DAAFFAB8487C25`，portable `352905FB4A42C334ACACFC849B6805EB2B8B30C45615F7959551CA7DDC218DD2`，raw exe `22BE67AC4394B86B825CDE418D47C45C08948E64A1129D1433D0EA0655DF3E9A`。
- 发布前验证：`pnpm vitest run tests/lib/codexMultiRouterSync.test.ts tests/lib/codexMultiRouterWizard.test.ts src/components/codex/CodexRouterWorkspacePage.test.ts`；`pnpm typecheck`；`pnpm exec prettier --check tests/lib/codexMultiRouterSync.test.ts src/components/codex/CodexRouterWorkspacePage.tsx src/components/codex/CodexRouterWorkspacePage.test.ts memory.md docs/release-notes/v3.16.4-7-zh.md package.json src-tauri/tauri.conf.json`；`git diff --check`（仅 Cargo.lock CRLF 提示）。
- 已知后续项：`main` 推送后的 GitHub Actions `Backend Checks` 失败在 Clippy，不是 GitHub release 资产上传失败；错误为 `src/commands/model_fetch.rs::fetch_models_for_config` 和 `src/services/model_fetch.rs::fetch_models` 参数数 `8/7` 触发 `clippy::too_many_arguments`，应单独把 Volcengine 参数收敛成结构体或配置对象后修复，避免混入已发布的 `v3.16.4-7` tag。

## 2026-07-02 Codex MultiRouter Catalog Route Divergence Misroute

- 另一位用户的日志包 `ccswitchmulti_logs_2026-07-02_141527-150209(1).zip` 证明了第三种 GPT 错路由形态：51 条 `request_model=gpt-5.5-longnows-gpt` 的 Codex 请求都落到 `codex-multirouter::route::router-codex-official`，实际上游模型被改写为 `model=gpt-5.5`；同时 `_codex_session` 侧仍记录 `gpt-5.5-longnows-gpt`，说明 Codex 选择器能看到 longnows alias，但 runtime 没有对应 exact route，只能被官方 `gpt` 前缀 route 接走。
- 这和“空 catalog 清掉 relay route”以及“双 route 重复 exact 名称”都不同。根因边界是工作台 `buildModelCatalogForRoutes` 曾经把 target provider 的 `modelCatalog.models` 全量写进 MultiRouter 聚合 catalog，即使当前 route 的 `match.models/prefixes` 接不住这些模型；例如 LongNows route 只声明 `claude-opus-4-8`/`claude`，但 catalog 仍暴露 `gpt-5.5-longnows-gpt`。
- 修复边界：聚合 catalog 只能投影当前 route 可 exact/prefix 匹配的可见模型；provider 模型刷新影响已保存 plan 时复用 `syncCodexMultiRouterPlanWithProviders`，让 `route.match.models`、`upstream.modelMap` 和 `modelCatalog.models` 同步重算，不再让工作台刷新路径和 provider 保存路径分叉。
- 回归测试：`src/components/codex/CodexRouterWorkspacePage.test.ts` 的“does not expose provider catalog models that no saved route can match”固定 LongNows Claude-only route 不得暴露 `gpt-5.5-longnows-gpt`；验证命令覆盖 `pnpm vitest run tests/lib/codexMultiRouterSync.test.ts tests/lib/codexMultiRouterWizard.test.ts src/components/codex/CodexRouterWorkspacePage.test.ts`。

## 2026-07-02 Codex MultiRouter Duplicate Exact Route Bidirectional Misroute

- 多人同日下午反馈“中转 GPT 走到官方”和“官方 GPT 走到中转”时，不要只沿空 `modelCatalog` 单向清空 alias 的链路排查；双向错路由的运行时根因是同一个 MultiRouter plan 里多条 enabled route 同时声明相同 exact 可见模型名，例如官方和第三方中转都保存了 `match.models=["gpt-5.5"]`。Rust `find_codex_route_by_match_priority` 无法从同一个 `request.model` 判断用户意图，只能按 routes 数组顺序取第一条 exact match，所以 route 顺序不同会产生两个方向的错路由。
- 真正修复边界在保存/同步层生成唯一可见模型名：官方/OAuth canonical source 保留原名，第三方/中转同上游模型必须变成 `gpt-5.5-<provider-suffix>` 这类 alias，并在 route 级 `upstream.modelMap` 写回 `{alias:"gpt-5.5"}`。`syncCodexMultiRouterPlanWithProviders` 现在先对当前 plan 实际引用的 provider 跑 `resolveWizardModelNameCollisions`，坏旧配置里 relay route 的裸 `gpt-5.5` 会被修成 `gpt-5.5-relay-gpt`；工作台 `handleSaveRoutingRoutes` 和 routes tab 自动刷新也会调用 `normalizeCodexRoutesForVisibleModelAliases` 修复手动编辑/旧配置绕过口。
- 运行时只增加诊断防线：当多个 route exact 命中同一模型时，`codex.rs` 会写 warning，提示 `ambiguous exact route match`、route ids 和“按顺序取第一条”。不要试图在 runtime 靠 provider 类型猜用户想走哪条；没有唯一可见模型名时，用户侧的选择信息已经丢失。
- 回归测试：`tests/lib/codexMultiRouterSync.test.ts` 覆盖 provider 同步修复官方/relay 同名 exact route；`src/components/codex/CodexRouterWorkspacePage.test.ts` 覆盖手动保存前修复重复 exact route；`src-tauri/src/proxy/providers/codex.rs` 的 `test_codex_router_duplicate_exact_routes_remain_order_dependent` 固定运行时顺序依赖和诊断语义。

## 2026-07-02 Codex MultiRouter Empty Catalog Relay GPT Misroute

- 多人反馈“本来走第三方中转的 GPT 请求被路由去官方”时，先查 provider 保存后的 MultiRouter 同步结果：`settingsConfig.codexRouting.routes[].match.models` 是否被清空、第三方 route 的 `upstream.modelMap` 是否丢失、聚合 `modelCatalog.models` 是否还包含 `gpt-*-relay` 这类可见别名。
- 根因不是 OAuth 代理与原生 Codex 差异，也不是 Rust runtime 主匹配优先级直接抢路由。直接触发点是 `c10a1541 fix(codex): sync multirouter catalogs after provider model edits` 引入的同步逻辑：`syncCodexMultiRouterPlanWithProviders` 把目标 provider 的空 `modelCatalog.models` 当成“用户删除了所有模型”，随后写入 `match.models=[]` 并删除 `upstream.modelMap`。relay route 失去可见 alias 后，运行时只能命中官方 GPT exact/prefix route。
- 修复边界：目标 provider 当前没有可用 catalog 时，不覆盖已保存 route 的 `match.models` / `modelMap`；`rebuildPlanModelCatalog` 在空目录回退路径复用 plan 里已有 catalog 条目的 `displayName`、`contextWindow`、`upstreamModel` 等字段。目标 provider 目录非空时仍按新目录同步新增/删除模型，并继续剪枝失效的 `spawnAgentModels`。
- 回归测试：`tests/lib/codexMultiRouterSync.test.ts` 的“目标 provider 目录暂时为空时不清空第三方 GPT 别名 route”覆盖官方 `gpt-5.5` 与第三方 `gpt-5.5-relay -> gpt-5.5` 并存、relay provider catalog 暂时为空、official catalog 正常更新时，relay route 和 spawn agent 候选必须保留。

## 2026-07-02 Volcengine AgentPlan OpenAPI Model Fetch

- 用户截图里的“目前火山引擎 Agentplan 获取不到模型”不是 API Key、网络或 `/models` 候选顺序的单点问题。火山 Agent Plan 的模型枚举文档是 `ListArkAgentPlanModel - 查询 Agent Plan 支持的模型列表`，在 `Agent Plan API` 管控面下；同页导航还有独立的 `ListArkCodingPlanModel - 查询 Coding Plan 支持的模型列表`。这类接口不是数据面 `https://ark.cn-beijing.volces.com/api/coding/v3/models` 或 OpenAI `/v1/models`。
- `origin/main`（2026-07-02 fetch 后为 `8d1b3306d09a27b9d8fc29694791d8421aba5f93`）没有修复 AgentPlan 专用模型枚举：全树无 `ListArkAgentPlanModel` / `ListArkCodingPlanModel`，只有通用 `/models` URL 候选和 UA 透传修正。不要把原版的通用 `model_fetch` 修复误判成火山 AgentPlan 支持。
- 纠偏：不要把“不能走 OpenAI `/models`”等价成“只能 catalog-only”，也不要把火山推理 API Key 和账号级 AK/SK 混为一类。火山 AgentPlan 有火山 AK/SK（存在 `meta.usage_script.accessKeyId` / `secretAccessKey`）时，应优先通过 `open.volcengineapi.com` 的 `ListArkAgentPlanModel` 管控面 OpenAPI 获取模型列表，并解析 `Result.Datas[].ModelID`；缺 AK/SK 但有推理 API Key 时，应继续尝试数据面 `/models`，失败再保留内置 `modelCatalog`；推理 API Key 和 AK/SK 都缺时才直接 catalog-only。
- 举一反三边界：AgentPlan 的“推理调用”仍是 OpenAI-compatible Chat/Responses 数据面，但“模型枚举”不是同一个接口。Provider 表单、MultiRouter 新向导、已保存路由页自动刷新三条入口都必须复用 plan-aware 获取逻辑；如果只修前两条，路由页进入 `routes` tab 时仍会用普通 `/models`，导致火山 AgentPlan 回归失败。自动刷新去重 key 也要把 OpenAPI action 与 AK/SK 的短哈希纳入比较，避免换 AK/SK 后复用旧失败状态。
- BytePlus 仍保持 catalog 回退，直到找到可验证的 BytePlus 专用模型列表接口契约；不要把 BytePlus 未证实地并入火山 CN OpenAPI。
- 不要通过把 `/api/coding/v3` 剥离成 `https://ark.cn-beijing.volces.com/v1/models` 之类猜测端点来“修复”。那只是换一个未证实的 OpenAI-compatible URL，和官方 `ListArkAgentPlanModel` 管控面边界不一致，容易把真实根因掩盖成另一个 404。

## 2026-07-01 Codex MiniMax Sensitive Image Media Retry

- 用户截图里的 `Provider: MiniMax; model: MiniMax-M3; upstream_status: HTTP 400; cause: input new_sensitive, messages[61]'s content[0] image is sensitive` 不是 MultiRouter route 物化丢能力；它说明 Codex 同一 session 历史里仍有图片块，上游 MiniMax 对某张图片做安全审核后拒绝。
- 不要把 `MiniMax-M3` 直接加入 text-only 预防名单，除非有供应商文档或实测确认它完全不支持图片。该错误措辞是图片安全拒绝，不是能力不支持；直接标 text-only 会永久关闭可能合法的图片输入。
- 修复边界在反应式 media retry：`forwarder.rs::media_retry_should_trigger` 已要求 adapter 是 Codex/Claude、整流开关开启、未重试过、原 provider body 确实含图片块；`media_sanitizer.rs::is_retriable_image_error` 再识别 unsupported image 与 MiniMax `base_resp.status_msg` / `new_sensitive` / `image is sensitive` 等图片审核错误，随后把图片块替换为 `[Unsupported Image]` 并对同一 provider 重试一次。
- 回归测试：`detects_minimax_sensitive_image_errors`、`reactive_triggers_for_codex_sensitive_image_errors`、`reactive_sensitive_image_error_still_requires_image_body`。后续排查类似 `messages[n].content[m] image is sensitive`，先看同 trace 是否有 `[Media] Image retry succeeded/still failed`，不要先猜 OAuth 或 MultiRouter catalog。

## 2026-07-01 CCSwitchMulti v3.16.4-6 Release

- v3.16.4-6 是 v3.16.4-5 后的热修正式发布，核心变更是 `fix(codex): strip invalid content from oauth response items`：Codex OAuth `/responses` 直透 ChatGPT backend 前会删除非 message/reasoning input item 上的冗余 `content`，避免 `Invalid input[3].content: array too long`。
- 版本面必须同步更新 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`，release note 是 `docs/release-notes/v3.16.4-6-zh.md`。
- 发布输出目录使用独立路径 `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.4-6`，不要复用默认“最新版ccswitchmulti”目录或清理旧 `output/release-*` / `scripts/logs/` 未跟踪目录。
- `v3.16.4-6` 已作为 BigStrongSun/ccswitchmulti 的 GitHub 正式 release 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-6`。Release 为非 draft、`prerelease=false`，并通过 `gh api repos/BigStrongSun/ccswitchmulti/releases/latest` 确认是 latest。
- Annotated tag `v3.16.4-6` 的 tag 对象为 `8e57b291468f3068189cd0725a6440170ee0527e`，解引用到版本提交 `e0614deb263f7b2dbcbb3b1cbe46162294d8a353`（`chore(release): bump CCSwitchMulti to v3.16.4-6`）。发布资产共 10 个：Windows setup、setup signature、portable zip、raw exe、`latest.json`、Linux/macOS build notes、README、`RELEASE-METADATA.md`、`SHA256SUMS.txt`。
- 发布后校验：`latest.json` 版本为 `3.16.4-6` 且指向 `v3.16.4-6` setup URL；raw exe 的 `FileVersion/ProductVersion` 为 `3.16.4-6`；GitHub asset digest 与本地 `SHA256SUMS.txt` 对齐，主资产 SHA256 为 setup `B0D69BA5610B3B3600F9E38B255A47AB444DFDEE8FF7469BDE0C906DE094C7DD`、portable `65B8EAB4F573199FFB212B89CF32D7679BA00740CD357D826AB924665CF2A3B0`、raw exe `F2E3941DBFD932E2B32A35B64168816FBF68806BEAD3EFCDBBFAD4D1F68757B4`。

## 2026-07-01 Codex Cross-Provider Model Switch Control Messages

- 跨 provider 继续同一个 Codex session 时，Codex Desktop 可能把 `/model` 切换产生的 system/developer 控制消息放进 Responses `input`。如果先前会话在 MiniMax-M3，之后切到 gpt-5.4 这类更严格的 Responses 上游，直接透传这些内部角色可能触发 HTTP 400；这不是 MiniMax 单独问题，而是同一会话历史跨协议/跨 provider 复用时的 input 形态问题。
- 已有 Chat 转换路径 `transform_codex_chat.rs` 会把 developer 映射为 system，并通过 `collapse_system_messages_to_head` 合并到首位，避免 MiniMax 对中途 system role 的严格校验失败；但原生 Responses 透传路径之前没有同等规整。
- 修复边界：`openai_compat.rs::normalize_codex_responses_passthrough_request` 会把 `input` 中 `type=message` 或缺省 type 且 `role=system/developer` 的控制消息提升到顶层 `instructions`，并从 `input` 删除这些内部控制消息。第三方原生 Responses 透传在 `forwarder.rs::should_normalize_codex_responses_passthrough_control_messages` 命中时调用；official OAuth normalizer 也先复用该逻辑，再执行 ChatGPT backend 专属的 tool output `content` 清理。
- 回归测试：`codex_responses_passthrough_promotes_control_messages_to_instructions`、`codex_oauth_responses_normalizer_promotes_control_messages_and_strips_tool_content`、`codex_responses_passthrough_control_message_normalizer_is_scoped`，并复跑 `responses_request_to_chat_normalizes_codex_internal_roles` 与 `responses_request_to_chat_merges_mid_stream_system_into_head` 确认 Chat 转换老路径未回退。

## 2026-07-01 Codex OAuth Responses Passthrough Content Shape

- 用户截图里的 `Invalid 'input[3].content': array too long. Expected an array with maximum length 0, but got an array with length 1 instead.` 发生在 Codex `/responses` 直透 ChatGPT Codex OAuth backend，日志特征是 `responses_to_chat=false`、`responses_to_messages=false`、`upstream_url=https://chatgpt.com/backend-api/codex/responses`。这不是第三方 Chat 转换问题，也不是模型容量问题。
- 根因是 `openai_compat::normalize_codex_oauth_responses_request` 对 Codex Desktop 已经发来的 `input` 数组只做字段补齐，未清理非 message item 上的冗余 `content`。官方 Codex app-server protocol 的 `function_call_output`、`custom_tool_call_output`、`tool_search_output` 只允许 `output/tools/call_id/status/execution` 等字段，不允许携带 message-style `content`；ChatGPT backend 会把该 content 视为长度必须为 0 的数组并返回 400。
- 修复边界：只在 official managed Codex OAuth passthrough normalizer 中清理 input item，保留 `message` 和 `reasoning` 的 `content`，删除 tool/call/web/image/compaction 等非 message item 的 `content`。公开 OpenAI Responses、第三方 Responses、Responses->Chat 转换路径不调用这条清理逻辑，避免扩大行为变更。
- 回归测试落点：`openai_compat.rs::codex_responses_request_normalizer_strips_content_from_tool_output_items` 覆盖 function/custom/tool-search output item 带 `content` 时会被删除，同时保留 `output/tools` 和普通 user message content。

## 2026-07-01 CCSwitchMulti v3.16.4-5 Formal Release

- CCSwitchMulti 正式发布远端是 `fork` (`BigStrongSun/ccswitchmulti`)；`origin`/`upstream` 指向原版 `farion1231/cc-switch`，发布、tag、asset upload 都不能推到 `origin`。
- `v3.16.4-5` 的发布语义是“本地最新改完的 `main` 作为正式版本”。如果发布准备后又产生发布质量修正（例如清理重复脚本键），必须先提交到 `main`，再把 annotated tag `v3.16.4-5` 移到最终 `HEAD`，然后推 `fork/main` 和强推 tag。
- `package.json` 曾经有两个相同的 `history:tool:check` script key。虽然值相同、行为不变，但会让 Vite/esbuild 在正式打包时提示 duplicate key；发布前必须只保留一个，验证用 `Select-String -Path package.json -Pattern '"history:tool:check"'`。
- 用户要求的流程是先创建 GitHub release，再打包和上传资产。release URL 是 `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-5`；创建 release 后如果 tag 被移动，需要 `gh release edit v3.16.4-5 --repo BigStrongSun/ccswitchmulti --latest --notes-file docs/release-notes/v3.16.4-5-zh.md` 重新确认 release 元数据。
- 本地正式打包建议使用独立目录 `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.4-5`，命令：`powershell -NoProfile -ExecutionPolicy Bypass -File scripts\local-release-pipeline.ps1 -ReleaseRoot C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.4-5 -Reason "manual-formal-release-v3.16.4-5"`。不要清理无关未跟踪目录，例如旧 `output/release-*` 和 `scripts/logs/`。

## 2026-07-01 Local Branch Merge Audit

- 本地 `main` 已吸收三个此前未合入的修复/功能分支：`codex/upstream-preserve-codex-live-settings`、`upstream/codex-history-repair-session-ui`、`upstream/codex-history-repair-session-manager`。
- `codex/upstream-preserve-codex-live-settings` 合并时冲突在 `src-tauri/src/codex_config.rs` 与 `src-tauri/src/services/proxy.rs`。正确处理是保留当前 main 的 MultiRouter、OAuth 登录态保护、CCSwitch 自有 catalog 指针清理、`PROXY_MANAGED` 陈旧 provider 表清理，同时补入旧分支的递归 TOML provider 优先合并逻辑。验证：`cargo fmt --manifest-path src-tauri\Cargo.toml --check`；`cargo test --manifest-path src-tauri\Cargo.toml codex_restore_ --lib -- --nocapture`。
- `upstream/codex-history-repair-session-ui` 是旧 upstream PR 形态；当前 main 已有更新的 CCSwitchMulti 历史修复实现。合并时不要用旧 UI 覆盖当前 `CodexHistoryRepairPanel`、`SessionManagerPage`、API types 或 MultiRouter repair command。最终只吸收 `.gitignore` 历史工具构建产物规则、`history:tool:check`、`scripts/codex-history-tool/build-windows-exe.ps1` 和 i18n key。验证：`pnpm history:tool:check`；`pnpm typecheck`；`cargo fmt --manifest-path src-tauri\Cargo.toml --check`。
- `upstream/codex-history-repair-session-manager` 的 glob state DB discovery / reconcile CLI 内容已经被当前 main 后续实现覆盖。冲突文件 `scripts/codex-history-tool/codex_history_tool.py` 与 `src-tauri/src/codex_history_migration.rs` 必须保留 main，避免旧默认 provider、较窄扫描逻辑和 mojibake 注释回滚当前实现。验证：`pnpm history:tool:check`；`cargo test --manifest-path src-tauri\Cargo.toml codex_history_migration::tests --lib -- --nocapture`。
- 合并后仍未进 `main` 的本地分支只剩非本轮目标：`backup/main-mixed-before-clean-20260622` 是旧混合备份，包含早期 sidecar/router 实验和 `Codex <codex@local>` 提交，不应直接合入；`docs/codex-responses-websocket-routing-notes` 只剩文档提交未合入，其中代码提交 `fix(codex): sync multirouter model cache` 已被 `main` 等价吸收。

## 2026-07-01 Codex OAuth Native Request Shape Diagnostics

- Added read-only script `scripts/codex-oauth-diagnostics.ps1` for capacity/OAuth triage. It writes sanitized evidence under `scripts/logs/codex-oauth-diagnostics/<timestamp>`: live Codex config, auth metadata, parsed `codex-router.log` events, capacity/error candidates, and summary. Token-like fields plus account ids are represented only by length and short SHA256 prefixes.
- Added `scripts/codex-request-shape-compare.mjs` for native-vs-proxy request shape diffing. It supports `--self-test`, `--serve-self-test`, file-based `--native/--proxy`, and mock-server mode with `--serve --native-command ... --proxy-command ...`. Mock mode exposes `CODEX_COMPARE_BASE_URL`, `CODEX_COMPARE_NATIVE_BASE_URL`, and `CODEX_COMPARE_PROXY_BASE_URL`; requests can be tagged with `x-codex-compare-side` or `?side=`.
- Added `docs/codex-oauth-native-diff.md` to fix the analysis order: first identify whether traffic is pure native, CCSwitchMulti local facade to official OAuth, or third-party routing; then compare `service_tier`, `prompt_cache_key`, `client_metadata`, `originator`, Responses-Lite, account id, and session/window ids; use Fiddler/mitmproxy only after source/log/mock diff cannot explain the behavior.
- Validation on this machine: Windows PowerShell 5.1 runs `codex-oauth-diagnostics.ps1`; the latest 100 parsed router events had no capacity/error candidates. Node `--self-test` and `--serve-self-test` both generated diff reports successfully.

## 2026-07-01 Codex Desktop Login Preservation During Takeover Restore

- Codex Desktop app 登录态的唯一安全来源是 live `~/.codex/auth.json`。CCSwitchMulti 的 MultiRouter/OAuth 只负责 LLM 请求出口，异常恢复、关闭接管、启动自恢复都不能把旧 `proxy_live_backup` 里的空 auth、API key auth 或过期 OAuth 快照覆盖到当前 live auth。
- 崩溃/系统重启/Codex 先于 CCSwitchMulti 启动的关键风险不是 `15721` 本地代理配置本身，而是恢复旧备份时删除或回滚 `auth.json`。`ProxyService::write_codex_live_verbatim` 现在在恢复 Codex 备份时，如果当前 live auth 有 OAuth 登录材料，只写/投影 `config.toml`，保留 live `auth.json`；第三方 API key 仍放进 `experimental_bearer_token`。
- 保持兼容边界：如果当前 live 没有 OAuth 登录材料，空 auth 备份仍可删除 `auth.json`，以支持 config-only 第三方 provider；有 live OAuth 时，空 auth 和 stale OAuth 备份都不能影响 app 登录。回归测试覆盖 `codex_restore_empty_auth_backup_preserves_current_live_oauth_login` 与 `codex_restore_stale_oauth_backup_preserves_current_live_oauth_login`。
- 日志证据要按时间线判断：`app-exit-events.jsonl` 里的 `abnormal_exit_detected` 后，`cc-switch.log` 会立即出现“检测到上次异常退出（存在接管残留）”“codex Live 配置已从备份恢复”“正在重新接管并补齐 Live”。这说明 Codex-only 启动时掉登录不是 Codex 启动瞬间被 CCSM 改了，而是旧版本 CCSM 上次恢复/重新接管时已经把坏 `auth.json` 留在 live 目录里。
- 额外审计边界：`services/provider/live.rs::LiveSnapshot::restore()` 当前是 `#[allow(dead_code)]` 且无生产调用者，但它原本也会原样写入或删除 Codex `auth.json`。为防止以后回滚/快照流程重新接入后复发，该路径也必须遵守同一规则：live auth 有 OAuth 登录材料时只恢复 `config.toml`，不写、不删、不回滚 `auth.json`。回归测试覆盖 `codex_live_snapshot_restore_empty_auth_preserves_live_oauth_login`、`codex_live_snapshot_restore_stale_oauth_preserves_live_oauth_login` 和无 live OAuth 时仍删除空 auth 的兼容行为。

## 2026-07-01 GitHub CI and Release Workflow Failure Boundaries

- GitHub CI 的 backend job 比本地常用验证更严格：`cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` 和 `cargo test --manifest-path src-tauri/Cargo.toml` 都会跑。发布前不能只跑前端 `typecheck/vitest/prettier`，Rust warning 在 stable toolchain 升级后会直接卡 CI。
- `tests/provider_service.rs::codex_official_to_deepseek_then_takeover_enters_and_restores_proxy_managed_live_config` 使用 proxy ephemeral port；断言不能写死 `15721`。正确判断是 live `config.toml` 指向 `http://127.0.0.1:<port>/v1` 且包含 `PROXY_MANAGED`，否则本地 Windows 和 GitHub cargo test 会误判失败。
- Release workflow 的 Linux 矩阵需要在 `pnpm tauri build --bundles appimage,deb,rpm` 前先构建 `codex-history-repairer`：`cargo build --manifest-path src-tauri/Cargo.toml --bin codex-history-repairer --features history-repairer --release`。该 bin 有 `required-features = ["history-repairer"]`，Tauri bundler 会寻找 sidecar，没预构建会报 `codex-history-repairer does not exist`。
- Release workflow 里 Windows x64 的失败点是 WiX `light.exe` 打包 MSI；主自动 release 已改成 x64 只构建/收集 NSIS `*-Windows-Setup.exe`，避免 WiX 阻塞整条 release。Windows ARM64 仍走 MSI，但 GitHub `windows-11-arm` runner 可能缺 `C:\Program Files\LLVM`，workflow 需要在缺失时 `choco install llvm -y --no-progress` 再写 `LIBCLANG_PATH`/`CLANG_PATH`。
- Release workflow 里 macOS 的失败点不是编译，而是 Apple signing secrets 缺失后仍把空 `APPLE_SIGNING_IDENTITY` 传给 Tauri，导致 codesign 报 `The specified item could not be found in the keychain`。正确降级是 build 阶段 unset 空的 `APPLE_SIGNING_IDENTITY`，后续只在 signing identity 存在时生成/公证 DMG；否则只发布 updater tarball 和 `.app.zip`。

## 2026-07-01 Codex MultiRouter Wizard Catalog Curation Flow

- Codex 单 provider 表单 `CodexFormFields` 的模型映射表第一列语义是“保留这个模型进入该 provider 的 modelCatalog”，不是子 Agent 候选。取消勾选会删除该模型行；上下箭头移动的是 catalog 行顺序。不要再在单 provider 获取 `/models` 后自动写 `spawnAgentModels` 前 5 个。
- MultiRouter 设置向导的正确顺序是：模型源 -> MultiRouter 命名 -> 配置检查 -> 获取/测试模型 -> 重名别名 -> 汇总模型排序/剔除 -> 路由预览 -> 子 Agent 候选排序 -> 保存发布。最终汇总页决定 `modelCatalog.models` 和 route `match.models` 保留哪些模型；子 Agent 页只从最终保留模型中选择最多 5 个并写入 `modelCatalog.spawnAgentModels`。
- `buildCodexMultiRouterWizardPlan` 支持可选 `planName`、`catalogModelOrder`、`spawnAgentModels`。传入 `catalogModelOrder` 时必须同时过滤 routes 和 final catalog，避免 UI 剔除模型但路由仍命中；传入 `spawnAgentModels` 时要过滤掉已剔除模型并限制最多 5 个。
- MultiRouter 向导进入“获取模型列表”步骤后必须逐个普通 Codex provider 重新调用 `/models`，不能因为已有 `modelCatalog` 自动跳过。每个 provider 卡片要显示读取中、成功有更新、成功无更新、跳过或失败状态；失败文案统一为“获取模型列表失败，请检查当前 provider 配置”，并同时进入向导问题面板。刷新后要保留“整理模型”里用户已取消/保留的勾选，只追加新增模型；`displayName` 缺省和显式等于模型 ID 在更新判断里等价，不应误报“有模型列表更新”。卡片点击应关闭向导并打开对应 provider 配置页。
- 普通 Codex provider 保存后需要同步引用它的已保存 MultiRouter 方案：route `match.models`、route `upstream.modelMap`、聚合 `modelCatalog.models` 和 `spawnAgentModels` 都要从 provider 最新 `modelCatalog` 重建。但同步时必须保留已保存 route 的可见别名和 `modelMap`，只用目标 provider 的最新目录补上下文、多模态等能力字段；不能直接用目标 provider 原始模型名覆盖 route，否则官方源和第三方中转暴露同名上游模型时会丢失路由区分。若同步导致 `spawnAgentModels` 中的模型被删除，只剪枝旧候选并提示用户点“处理”进入对应 MultiRouter routes 页人工补选，不要自动按 catalog 前 5 个补齐。

## 2026-07-01 Codex Provider Protocol Probe Concurrency

- Codex provider 表单的“测试 Chat / Responses”可以并发，但不要改成无界并发：`src/components/providers/forms/CodexFormFields.tsx` 使用 `CODEX_PROTOCOL_PROBE_MODEL_CONCURRENCY = 3` 的模型级并发池，避免大模型目录串行太慢，也避免一次性打爆真实供应商限流。
- 每个模型内部 Responses 和 Chat 两个协议用 `Promise.all` 同时探测；每完成一个模型就更新“模型映射”行级协议 tag，最终汇总仍按原 catalog 顺序输出，避免并发完成顺序影响用户看到的模型列表。
- 回归测试落点是 `tests/components/CodexFormFields.test.tsx` 的 bounded model concurrency case：初始只允许前三个模型同时发起 Responses/Chat 探测，任一模型完成后才开始下一个模型。

## 2026-06-30 Codex GLM Model Context and Probe Guidance

- 2026-07-01 用户用智谱 GLM key 实测“测试 Chat / Responses 全不通”时，真实网络和 key 都正常：`https://open.bigmodel.cn/api/paas/v4/chat/completions` 与 `https://open.bigmodel.cn/api/coding/paas/v4/chat/completions` 能返回 200，`/api/paas/v4/models` 和 `/api/coding/paas/v4/models` 也能返回模型列表；失败的 UI 报错路径是 `.../api/coding/paas/v4/v1/chat/completions` / `.../v4/v1/responses`，根因是新加的协议探测 URL 构造没有复用 `/models` 的版本段规则。修复边界：`probe_codex_responses_for_config` / `probe_codex_chat_for_config` 对已以 `/vN` 收尾的 Base URL 直接拼 `/responses` / `/chat/completions`，避免再追加 `/v1`；智谱仍只有 Chat Completions 路径可用，Responses 路径按官方当前接口返回 404，应由 UI 标为 Chat 可用而不是“全不通”。
- 2026-07-01 同类 URL 风险不只智谱：Codex 预设中火山 Agentplan `https://ark.cn-beijing.volces.com/api/coding/v3`、BytePlus `https://ark.ap-southeast.bytepluses.com/api/coding/v3`、DouBaoSeed `https://ark.cn-beijing.volces.com/api/v3` 也属于“非 `/v1` 版本化根地址”，旧协议探测会错拼成 `/v3/v1/...`。普通以 `/v1` 收尾的供应商旧逻辑已经能直接拼 endpoint，不属于这次 bug。回归测试要同时覆盖 `/v4` 智谱和 `/v3` 火山/豆包代表路径。
- 2026-07-01 Codex provider 的“测试 Chat / Responses”不能只在按钮旁显示一条错误摘要；真实问题是每个模型可能有不同协议能力。`CodexFormFields` 的探测结果应按模型保存并在“模型映射”每行显示小 tag：`双协议`、`Responses`、`Chat`、`不可用`，tag title 保留 Responses/Chat 详细返回。汇总文案要列出双协议通过、仅 Responses、仅 Chat、双协议失败的模型，避免用户只看到第一个失败模型（如 `glm-4.5`）而不知道其它模型状态。
- 2026-07-01 “下一步”必须由真实探测结果驱动而不是模型名启发式：双协议通过和仅 Responses 通过的模型进入 Responses provider，只有 Chat 通过的模型进入 Chat provider，双失败模型不进入拆分建议。拆分成两个 provider 只在新增 provider 场景有 `onProviderSplitSuggestionChange` 回调时弹确认框；编辑已有 provider 时只显示行级协议 tag 和汇总，不弹一个确认后无实际保存效果的拆分对话框。
- 2026-06-30 复查用户提供的智谱 Coding Plan key 后确认：`https://open.bigmodel.cn/api/coding/paas/v4/models` 当前返回的 GLM 条目只有 `id/object/created/owned_by`，没有 `context_window`、`max_context_length` 等规格字段；因此自动获取模型列表若要补齐 GLM 上下文，不能只靠 `/models`。官方 Mintlify 文档提供稳定 markdown：`/cn/guide/start/model-overview.md` 的模型表给出 GLM-5.2 `1M`、GLM-5.1/5/5-Turbo/4.7/4.6 `200K`、GLM-4.5-Air `128K`，单模型页如 `/cn/guide/models/text/glm-4.5.md` 给出 GLM-4.5 `128K`。后端 `model_fetch.rs` 的正确策略是：先解析 `/models` 显式 metadata；若智谱/Z.AI endpoint 的 GLM 模型缺上下文，再从官方 docs markdown 补齐，且只填缺失值、不覆盖上游显式值。
- 2026-06-30 后续完善为分层上下文补齐：`model_fetch.rs::enrich_missing_context_windows` 是统一入口，优先保留 `/models` 显式字段，其次 provider 官方 resolver（当前智谱 docs markdown），最后才用 `https://models.dev/api.json` 的 `limit.context` 作为公共目录兜底。models.dev 不能全局按模型名匹配，必须先确认当前成功 `/models` endpoint 以该 provider 的 `api` 前缀开头，再在该 provider 的 `models` 对象里匹配 key/id 或唯一后缀；否则 OpenRouter/Requesty 这类聚合目录中的同名模型容易串供应商。
- 2026-06-30 分层补齐的不变量已集中到 `model_fetch.rs::apply_missing_context_windows`：所有 provider 官方 resolver 和公共目录 fallback 都必须通过它写回，确保只填 `context_window=None` 的模型，永远不覆盖 `/models` 显式 metadata。新增 resolver 时同步补三类测试：显式值不被覆盖、provider/模型识别边界、官方文档解析退化场景。
- 智谱 `/models` 端点会返回模型上下文元数据时，Codex provider 自动获取模型列表必须优先从后端 `model_fetch.rs::extract_context_window` 解析真实字段；不要在前端为 GLM 写静态上下文表。应持续补充 `/models` 解析字段，比如 `context_length`、`max_context_length`、`max_input_tokens`、`limit.context`、`limits.context`、`metadata.maxContextLength` 等。
- 前端 `resolveFetchedCodexModelContextWindow` 的正确优先级仍是：远端 `/models` 显式值 > 用户已有 catalog 值 > 少量历史保守兜底（如 DeepSeek alias / preset 已有值）。如果某个供应商实际返回了上下文字段但 UI 为空，先修 `model_fetch.rs` 字段解析。
- Codex 表单的“测试 Chat / Responses”依赖模型目录；如果 catalog 为空，不能只 toast “请先获取模型列表”。正确交互是展开高级选项，滚到“模型映射”，聚焦并高亮右上角“获取模型列表”按钮，同时在确认框和提示文案里明确测试前需要先获取/添加模型。

## 2026-06-30 Release Pipeline Notes

- `scripts/local-release-pipeline.ps1` 会在 `scripts/export-latest-ccswitchmulti.ps1` 导出后追加 `RELEASE-METADATA.md`，因此必须在写完 metadata 后重新生成 `SHA256SUMS.txt`，否则 release 资产里的校验和会漏掉 metadata 文件。后续修改发布流程时要保持这个顺序：build/export -> metadata -> checksums -> upload。

## 2026-06-30 UI Portal Layer Ordering Audit

- Codex MultiRouter 向导相关的“点击后像卡死”不只来自单个 Dialog：全屏 provider panel 可到 `z-[140]`，但共享 Radix `SelectContent`/`PopoverContent`/`TooltipContent` 之前停在 `z-[100]`，`DropdownMenuContent` 甚至是 `z-50`，都会在向导上方打开 provider 表单时被面板遮住。
- 统一层级落点在 `src/components/ui/layers.ts`：普通 dialog `40/50/60`，portal 浮层 `z-[180]`，top dialog `z-[200]`，top dialog 内部需要 portal 的下拉用 `z-[210]`。不要再在业务组件里随手写 `z-[1000]` 或 `z-[200]` 抢层级；需要例外时先判断它属于普通 panel 浮层还是 top dialog 内浮层。
- `CodexMultiRouterWizard` 内部连通性测试确认框必须是 `zIndex="top"`，因为它从 `z-[120]` 的 wizard portal 内再打开 Radix dialog；如果仍用 `alert z-[60]`，确认框会被向导自身遮住。
- models.dev 定价导入弹窗本身是 `zIndex="top"`，里面的 provider `SelectContent` 是 body portal，必须显式使用 `UI_LAYER_CLASS.topDialogFloating`，否则默认普通浮层 `z-[180]` 仍会落在自己的 top dialog 下方。

## 2026-06-30 Dialog Top Layer Above Codex Wizard Provider Panel

- 用户截图显示 Codex provider 表单里点击“测试 Chat / Responses”后按钮旁出现“已打开测试确认框”，但确认 Dialog 仍不可见。根因不是 click/state 没触发，而是层级定义错误：MultiRouter 向导打开时 `AddProviderDialog` 的 `FullScreenPanel` 会提升到 `z-[140]`，而通用 Dialog 的 `zIndex="top"` 只有 `z-[110]`，仍在全屏 provider 面板下面。
- 修复边界：把 `src/components/ui/dialog.tsx` 的 `top` 层级统一提高到 `z-[200]`，同时 overlay 和 content 都使用同一 top 层级；不要只给单个 `CodexFormFields` content 加 class，否则 overlay/portal 层仍可能被面板遮住。回归测试：`tests/components/CodexFormFields.test.tsx` 断言协议测试确认框使用 `z-[200]`。

## 2026-06-30 Codex MultiRouter Single Source Entry Scroll Fix

- 用户反馈在“创建多路路由”里点击“单独接入模型源”后会立刻滚到 API Key / 高级选项区域，必须再滚回顶部选择模型源。根因是 `ProviderForm` 新建 Codex provider 时默认 `selectedPresetId="codex-0"`，挂载后的自动 `handlePresetChange("codex-0")` 也调用了此前为“手动选择预设后滚到配置字段”新增的 `scrollCodexProviderDetailsIntoView()`。
- 正确边界：打开单独接入模型源表单时，自动应用默认 OpenAI/Codex 预设只是为了给表单一个可编辑初始状态，不应改变滚动位置；用户手动点击具体模型源预设（如 DeepSeek、Zhipu GLM）后仍应滚到 API Key/Base URL 字段，避免停留在高预设网格误判无响应。
- 修复点：`handlePresetChange(value, { scrollDetails?: false })` 支持禁止滚动；仅 `useEffect` 的默认 `codex-0` 自动应用传 `scrollDetails:false`。回归测试在 `tests/components/ProviderForm.codexPreset.test.tsx` 同时覆盖初次挂载不滚、手动选择预设仍滚。

## 2026-06-30 Codex MultiRouter Duplicate Upstream Model Alias Routing

- 普通 Codex provider 表单里的“同名模型重命名”本质不是只改 `displayName`：能让 Codex Desktop 和后端路由区分两个同上游模型的是 `model` 可见 alias，`displayName` 只是菜单展示文本。`upstreamModel` 才保存真实上游模型名；后续排查不要把 displayName 当作可路由 key。
- 根因分两层：前端手动 MultiRouter 工作台仍按 visible `model` 聚合并去重，没有像向导一样先对重复 `upstreamModel` 生成稳定 alias；后端 `targetProviderId` 物化路径还会从目标 provider 复制 settings，丢掉 MultiRouter plan 上的 `modelCatalog`，导致 alias -> upstreamModel 查找失效。
- 修复边界：`CodexRouterWorkspacePage` 在创建 routing plan、打开候选 route、保存 route、刷新模型后重建 plan catalog 时复用向导的 `resolveWizardModelNameCollisions`，对第三方/中转同上游模型生成 `模型名-provider名` alias；工作台聚合 catalog 只在 alias 与上游名不同时写 `upstreamModel`，保持非 alias 条目紧凑。
- route 保存必须写入 route 级 `upstream.modelMap`，例如 `{"gpt-5.5-relay-gpt":"gpt-5.5"}`。这是因为运行时会物化 target provider，不能只依赖普通 provider 自己的原始 catalog；向导 `buildWizardRoutesFromSources` 也同步写 modelMap，避免 UI 显示 alias 但请求把 alias 发到上游。
- 后端修复点：`src-tauri/src/proxy/providers/codex.rs::materialize_codex_routed_provider_from_target` 必须保留 route provider 的 `modelCatalog`，让 `apply_codex_request_upstream_model` 能通过 visible alias 查回真实上游模型。回归测试：`test_materialize_routed_provider_preserves_model_catalog`。
- 前端回归测试：`src/components/codex/CodexRouterWorkspacePage.test.ts` 覆盖官方 `gpt-5.5` 和第三方 `gpt-5.5` 同时进入手动 MultiRouter 后，第三方变为 `gpt-5.5-relay-gpt`、plan catalog 保留 `upstreamModel: gpt-5.5`、route match 使用 alias 且 `upstream.modelMap` 指回真实模型。

## 2026-06-30 Codex GLM 5.2 Responses-To-Chat 400 Diagnostics

- 用户截图分析 Codex 26.623.61825 子 agent 经 CCSwitchMulti `/responses` 转 Zhipu GLM `/chat/completions` 后返回 HTTP 400，且日志里 `header_count=16` 失败较多。排查确认当前 `codex_router_log` 的 `header_count` 来自 `ordered_headers.len()`，是 HTTP 请求头数量，不是转换后 Chat body 顶层字段数量，不能直接按“body 字段数 16”归因。
- 上游原版也有同类公开 issue：farion1231/cc-switch `#4792` 在 v3.16.4 上报 `Provider: Zhipu GLM5.2; model: glm-5.2; upstream_status: HTTP 400; cause: messages.content.type 参数非法，取值范围 ['text']`；`#4465` 也指向 Codex Responses->Chat 转换链在图片/content part 上的兼容问题。对比 `origin/main`，原版仍会把 GLM 输入里的 `input_image` 转为 Chat `image_url` content part，且没有 request_shape 日志，因此用户看到的现象确实不是 Multi fork 独有。
- 对应根修：`glm-5.2` 这个文本模型 / coding endpoint 按 text-only 处理。`src/config/codexProviderPresets.ts` 的 Zhipu GLM / Zhipu GLM en `modelCatalog` 声明 `inputModalities=["text"]`、`textOnly=true`、`supportsImage=false`；`src-tauri/src/proxy/providers/transform_codex_chat.rs::codex_chat_model_is_text_only()` 也按模型名兜底识别明确文本模型（如 `glm-5.1`、`glm-5.2`、`glm-5-turbo`），保证旧配置或缺少 route capability 时仍把图片 content part 降级成文本占位，避免 `messages.content.type` 非法。不要把这条规则扩大到 `glm-5v-*` 等视觉模型。
- 根修边界在 Codex Responses->Chat 转换链：`src-tauri/src/proxy/providers/transform_codex_chat.rs` 不再默认透传 `metadata` 和 `service_tier` 到第三方 Chat 兼容上游；这两个字段对 GLM/DeepSeek/Qwen 等自动缓存或普通 Chat 路径没有价值，且容易被严格兼容层拒绝。`stream_options.include_usage` 暂时保留，因为它支撑第三方流式 usage 记账，不能无证据全局移除。
- 400 可观测性补强：`src-tauri/src/proxy/forwarder.rs` 在 Codex `responses_to_chat=true` 时给 `request_prepared` 和 `upstream_error` 追加脱敏 `request_shape`，只记录 `top_keys`、`messages` 数量、`tools` 数量/类型、`thinking`/`reasoning_effort`/`stream_options`/`parallel_tool_calls` 等字段形态，不记录 prompt、工具参数、API key 或工具函数名。后续遇到空 `body_summary` 的 400，应先看同 trace 的 `request_shape`，不要靠截图猜字段。
- GLM 5.2 thinking 配置依据官方文档校正：智谱“迁移至 GLM-5.2”文档明确列出 `thinking={"type":"enabled"}` 和 `reasoning_effort="max"` 示例，因此 Codex 预设和后端推断都保留/补齐 `supportsEffort=true`、`effortParam="reasoning_effort"`、`effortValueMode="deepseek"`。不要仅因 z.ai 返回空 400 就回退成 `effortParam=none`；若后续 request_shape 证明某个 coding endpoint 拒绝该字段，再做 provider/endpoint 级 capability，而不是全局关掉 GLM effort。
- 回归测试落点：`responses_request_to_chat_drops_responses_only_metadata_and_service_tier`、`responses_request_to_chat_downgrades_images_for_glm_5_text_models`、`responses_request_to_chat_keeps_images_for_glm_5v_vision_models`、`test_resolve_codex_chat_reasoning_infers_glm_5_2_effort_support`、`codex_chat_request_shape_omits_prompt_text_and_records_field_shapes`、`tests/config/codexChatProviderPresets.test.ts` 的 GLM text-only / effort 预设断言。已验证：`cargo test --manifest-path src-tauri\Cargo.toml responses_request_to_chat_ --lib`、新增 Rust 单测、`pnpm vitest run tests/config/codexChatProviderPresets.test.ts`、`pnpm typecheck`、`cargo fmt --manifest-path src-tauri\Cargo.toml --check`、`git diff --check`。

## 2026-06-30 Codex Provider Chat / Responses Probe Visibility Fix

- 用户截图反馈“创建多路路由 -> 单独接入模型源 -> 高级选项 -> 测试 Chat / Responses”点击后像页面卡死。该按钮属于 `ProviderForm(appId="codex") -> CodexFormFields`，不是 `CodexMultiRouterWizard`。根因之一是 AddProviderDialog 使用 `FullScreenPanel`，默认层级 `z-[60]`，而协议测试确认 Dialog 也使用 `zIndex="alert"` 即 `z-[60]`；同层级 portal/动画 stacking context 下，确认框可能被全屏面板压住，用户看不到任何反馈。
- 修复边界：只改 Codex provider 的协议探测交互。`CodexFormFields` 的确认 Dialog 改为 `zIndex="top"`，点击“测试 Chat / Responses”立即在按钮旁显示“已打开测试确认框”；确认后先显示正在测试的模型数量和当前进度；成功/警告/错误分别使用不同文本颜色；catch 到后端 invoke/网络异常时保留内联 `role="alert"` 错误，并恢复按钮可点，避免只靠 toast 或控制台导致用户误判卡死。
- 回归测试落点：`tests/components/CodexFormFields.test.tsx` 覆盖确认 Dialog 顶层 `z-[110]`、点击后即时状态提示、后端异常时内联 alert 和按钮恢复。相关验证：`pnpm vitest run tests/components/CodexFormFields.test.tsx tests/components/ProviderForm.codexPreset.test.tsx`、`pnpm typecheck`、`pnpm build:renderer`、`git diff --check`。

## 2026-06-30 Codex MultiRouter Wizard Small Window Layout Fix

- 用户截图显示 Codex MultiRouter 向导在较小窗口、尤其接近 Tauri 默认 `1000x650` / 最小 `900x600` 时底部按钮区被裁掉。根因不是向导状态机或步骤跳转，而是 `CodexMultiRouterWizard` 中间 grid 使用 `max-h-[82vh]`，但外层没有整体高度约束，最终高度约等于 header + `82vh` + footer，默认高度下会超过视口。
- 修复边界：只调整 `src/components/codex/CodexMultiRouterWizard.tsx` 的遮罩弹窗布局。外层 overlay 改成 flex 居中并 `overflow-hidden`，向导 shell 改为 `max-h-full flex flex-col min-h-0`，header/footer `shrink-0`，中间 body `flex-1 min-h-0 overflow-hidden`，右侧内容和左侧步骤栏各自滚动。不要通过单纯提高 `tauri.conf.json` 默认窗口高度来掩盖问题，因为用户仍可能缩小窗口或恢复旧窗口状态。
- 回归测试落点：`tests/components/CodexMultiRouterWizard.test.tsx` 断言向导 shell/body/footer 的关键布局类，避免后续把高度约束重新放回 `82vh` 内容区。验证命令：`pnpm vitest run tests/components/CodexMultiRouterWizard.test.tsx`、`pnpm typecheck`、`pnpm build:renderer`、`git diff --check`。浏览器直接打开 Vite renderer 会因缺少 Tauri IPC 出现 `window.__TAURI__` 相关错误，只能作为有限页面加载检查；完整交互仍以 Tauri 桌面或组件测试为准。

## 2026-06-30 Codex MultiRouter Post-Setup Validation Refresh

- 用户追问“配置完返回的校验和刷新流程”，排查范围仍只限 Codex MultiRouter。现有成功判定已经正确：`StatusTab` 只在本地代理运行、Codex 接管、当前 provider 是选中 MultiRouter、入口/规则启用，并且当前方案 route 有真实成功转发证据时才触发 `onRuntimeReady`，不会因为其它 Codex 请求 200 就提前进入历史修复。
- 新发现的刷新体验缺口：向导 finish 页启用 MultiRouter 后，父级只 `invalidateQueries` proxyStatus/proxyTakeoverStatus/providers，状态页要等轮询或后台 refetch 才显示最新监听/接管/current provider/日志；用户配置完返回后可能短时间看到旧校验状态。修复为启用后显式 `refetchQueries` proxyStatus、proxyTakeoverStatus、providers/codex 和 usage/logs。
- 状态页新增“刷新校验”手动入口：刷新同一组校验源并显示完成/失败提示，便于用户从 Codex 发出一次请求后立即重新检查链路卡片、最近转发和历史修复触发条件。不改变成功判定、route 归因、诊断探测或模型源刷新逻辑。
- 继续排查其它 Codex 跳转入口后发现：工具栏直接打开 Codex MultiRouter 时如果只 `setCurrentView("codexRouter")`，会沿用上一次 `codexRouterWorkspaceTarget` 的 provider/tab。修复为统一走 `openCodexRouterWorkspace(null, "status")`；工作台内部 tab 跳转新增滚动容器 `scrollTo({ top: 0 })`，避免从长页面切到状态/模型源/测试页时停留旧滚动位置。历史修复跳转已有 `SessionManagerPage` 测试覆盖一次性消费，不需要改。

## 2026-06-30 Codex MultiRouter Configuration Guide Navigation Audit

- 用户明确收窄排查范围：只处理 Codex 分支配置和 Codex 多路路由配置指南，不泛化到 Claude Desktop、Gemini、OpenCode、OpenClaw 或 Hermes。上一轮误扩展改动已撤回，当前只沿 `CodexRouterWorkspacePage` / `CodexMultiRouterWizard` / Codex provider 表单查同类“点击后状态已变但用户看不到下一步”的问题。
- 新发现的真实 Codex-only 缺口：`CodexRouterWorkspacePage` 已从 `App` 接收 `onCreateProvider`，但参数被命名为 `_onCreateProvider` 后没有使用；在工作台没有普通 Codex 模型源、RouteCandidatePicker 为空时，只提示“先添加至少一个 Codex 模型源”并提供“关闭”，用户无法从配置指南链路直接打开 Codex 模型源添加面板。
- 修复边界：只在 Codex MultiRouter 工作台接入已有 `onCreateProvider` 回调，给“模型源”页和“没有可选 router”空状态提供“添加模型源”；同时给行内展开的候选 router 选择器 / MultiRouter 设置面板加 `scrollIntoView({ behavior: "smooth", block: "nearest" })`，避免点击“编辑匹配规则/创建多路路由”后面板在视口下方导致再次误判为卡死。不改 route 保存、provider 协议、modelCatalog、wizard 状态机或非 Codex app。
- 回归测试落点：`src/components/codex/CodexRouterWorkspacePage.test.ts` 覆盖无模型源时点击“添加模型源”会触发父级回调，以及打开“编辑匹配规则”后滚到行内 RouteCandidatePicker。

## 2026-06-30 MultiRouter Provider Preset Click Perceived Freeze Fix

- 用户截图里“创建多路路由 -> 单独接入模型源”点击 `Zhipu GLM` 后并不是走 `UniversalProviderPanel`，而是 `ProviderForm(appId="codex") -> ProviderPresetSelector -> handlePresetChange("codex-<index>")`。`Zhipu GLM` 来自 `src/config/codexProviderPresets.ts`，不在 `universalProviderPresets`。
- 只读追踪确认 GLM 预设切换会一次性批量更新 Codex auth/config/modelCatalog/reasoning/takeover/form reset，但 `CodexFormFields` 的 catalog/routing 父子回写已有 ref + JSON 比对守卫，没有发现无限 setState 循环。用户感知“卡死”的主要交互根因是预设网格很高，点击后仍停在预设按钮区，API Key/Base URL/模型映射等实际要填写的字段在下方首屏外，视觉上像没有继续。
- 修复边界：`src/components/providers/forms/ProviderForm.tsx` 给 Codex 字段区加 `ref`，在 Codex 新建模式选择任意预设或自定义模型源后，用 `setTimeout(0)` 等 React 提交完成再 `scrollIntoView({ behavior: "smooth", block: "start" })`，把视口带到关键配置区；不改 GLM 协议、modelCatalog、reasoning 或保存逻辑。
- 回归测试：`tests/components/ProviderForm.codexPreset.test.tsx` 直接渲染 `ProviderForm appId="codex"`，先后点击 `DeepSeek` 与 `Zhipu GLM` 两个不同 Codex 预设，断言对应 Base URL、GLM 的 `glm-5.2` catalog、本地接管开启，并确认每次选择都触发滚动；配合 `tests/components/CodexFormFields.test.tsx` 验证 catalog/routing 受控回写仍稳定。

## 2026-06-30 Promote v3.16.4-4wizard To Latest Release

- 用户在 GitHub releases 列表截图里看到 `v3.16.4-3` 仍显示 `Latest`，原因不是 `main` 未推送，而是 `v3.16.4-4wizard` 仍标记为 prerelease。GitHub 只会把 `Latest` 标给正式 release，prerelease 即使发布时间更新也不会替代正式版 latest。
- 已将 BigStrongSun/ccswitchmulti 的 `v3.16.4-4wizard` release 从 prerelease 改为正式 release，并通过 `gh release edit ... --latest` 显式标记为 Latest；该 tag 的 peeled commit 已在上一轮指向 `main` 的 `15c2d728ae04b857a5531859ce911de6c2665b57`。后续判断“页面看不到 latest”时要先检查 `isPrerelease` 和 GitHub 的 explicit latest 标记，不要只看 tag/main 是否推送。

## 2026-06-29 Merge Wizard Trial Branch Into Main

- 用户纠正 `v3.16.4-4wizard` 不能只停留在 `codex/multirouter-wizard` 分支：需要把 wizard 试用线合入 `main`。合并前确认 `main...codex/multirouter-wizard` 为 `0 41`，即 `main` 没有独有提交、wizard 分支领先 41 个提交。
- 合并策略：在 `main` 上执行非快进 merge `codex/multirouter-wizard`，保留一个明确 merge commit，避免 `main` 静默快进导致后续溯源不清。未跟踪的 `docs/release-notes/v3.16.4-4-zh.md` 和 `scripts/logs/` 仍保持未跟踪状态，不纳入合并或提交。
- 合入内容包括 MultiRouter 向导、协议/缓存能力修复、Hermes/usage upstream bugfix、Windows 图标根修复、网站图标导出以及 `3.16.4-4wizard` 版本面。后续若刷新 `v3.16.4-4wizard` release，tag 应指向 `main` 上的 merge/memory 提交，而不是仅指向 wizard 分支提交。

## 2026-06-29 MultiRouter Wizard Provider Config Protocol Display Fix

- 用户试用向导时反馈配置核心参数页误导：`OpenAI Official Backup` 显示“API 格式：未显式设置，向导保存路由时默认 Chat Completions”，同时“已有模型目录”provider 显示“未配置在线获取参数”。根因是 `CodexMultiRouterWizard` 配置页直接读 `meta.apiFormat/settingsConfig.apiFormat` 和本地 `/models` 抓取参数，没有复用 `inferWizardApiFormat()` 这条已经用于保存 route 的事实来源。
- 修复边界：配置页的 API 格式展示必须和最终 route 生成一致。官方 OpenAI/OAuth 或暴露 GPT/O 原生 Responses 模型的 provider，即使旧 metadata 写 `openai_chat` 或未写，也显示/保存为 `Responses API`；未知第三方没有实测结果时才保守显示 Chat Completions。若旧配置和推断不同，UI 要明确说“向导推断，已覆盖旧配置”，避免用户以为 official 会走 Chat。
- `/models` 获取能力和可路由模型目录是两回事：没有 Base URL/API Key 只代表不能在线刷新模型列表，不代表不能生成路由。已有 `modelCatalog` 的 provider 应显示“已有 modelCatalog，可跳过 /models 在线读取；如需刷新再补 Base URL/API Key”，不要再写“未配置在线获取参数”这种像错误的提示。
- 回归测试：`tests/components/CodexMultiRouterWizard.test.tsx` 覆盖 catalog-only provider 不显示旧提示，以及 `OpenAI Official Backup` 带旧 `openai_chat` metadata 时配置页展示 `Responses API` 并提示覆盖旧配置；数据层已有 `inferWizardApiFormat` 测试覆盖最终 route 保存为 `openai_responses`。

## 2026-06-29 CCSwitchMulti v3.16.4-4wizard Iconfix Release Refresh

- 按用户要求将既有 GitHub prerelease `v3.16.4-4wizard` 更新到任务栏图标根修复后的最新提交。先推送 `codex/multirouter-wizard`，再强制移动 annotated tag `v3.16.4-4wizard`，确保 peeled tag 指向最新提交而不是旧 `ea455656...`。
- Windows 资产使用 `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.4-4wizard-iconfix-assets` 覆盖上传：setup、setup.sig、portable zip、raw exe、`latest.json`、`README.md`、`RELEASE-METADATA.md`、Linux/macOS build notes 和合并版 `SHA256SUMS.txt`。既有 macOS `.dmg/.app.zip` 资产保留，因为 Windows 主机未重建 macOS 包。
- 上传后验证：远端 `refs/tags/v3.16.4-4wizard^{}` 指向任务栏图标修复提交；关键 Windows asset digest 为 setup `4a1b3edc...`、portable `10c7eaa5...`、raw exe `248cb88b...`、latest.json `15f54d63...`。`SHA256SUMS.txt` 同时包含新 Windows 资产和保留的 macOS 资产 digest。

## 2026-06-29 Windows Taskbar Icon Embedded Resource Root Fix

- 用户再次截图反馈任务栏图标仍像白色圆团后，重新沿真实链路排查：`src-tauri/icons/icon.ico` 已更新且小帧仍是 DIB，但 `src-tauri/target/release/cc-switch.exe` 与 `%LOCALAPPDATA%\CCSwitchMulti\cc-switch.exe` 的 `ExtractAssociatedIcon()` hash 仍是旧值。根因不是安装覆盖失败，而是 Cargo/Tauri 构建复用了旧 `resource.lib`，`build.rs` 没有声明 `icons/icon.ico`/相关 PNG 为 `rerun-if-changed`。
- 修复：`src-tauri/build.rs` 显式声明 `icons/icon.ico`、`32x32.png`、`128x128.png`、`128x128@2x.png` 和 `tauri.conf.json` 的构建依赖。修改图标后必须触发 native resource 重新生成，不能只看 `icon.ico` 文件时间。
- 图标生成策略调整：`scripts/generate-windows-icons.py` 不再手绘一套任务栏小图；`16/24/32/48/64` 也从 `assets/brand/ccswitchmulti-codex-app-icon-1024.png` 缩放生成，保持任务栏、安装器、应用内和网站图标同一个品牌 master。ICO 小帧仍写入 32-bit DIB，保留 Windows shell 兼容性。
- 安装链路补强：`src-tauri/nsis/installer-hooks.nsh` 继续修开始菜单/桌面快捷方式，并额外修存在时的任务栏固定项 `CCSwitchMulti.lnk`；`scripts/verify-windows-install-icon.ps1` 也检查可选任务栏固定项。
- 验证：运行 `python scripts/generate-windows-icons.py` 和 `python -m py_compile scripts/generate-windows-icons.py`；执行 `cargo clean --manifest-path src-tauri\Cargo.toml -p cc-switch` 后重新 `scripts\export-latest-ccswitchmulti.ps1 -ReleaseRoot C:\Users\sunda\Documents\LLMservice\ccswitchmulti-iconfix-install`，确认源 `icon.ico`、`src-tauri\target\release\cc-switch.exe`、raw exe 的 associated icon hash 全部为 `3E1F633E16DBD922D6A7A4538B1450276803745DDCC20C0BE5F8AB3875A9F3E5`；静默安装后 `scripts\verify-windows-install-icon.ps1` 通过，安装目录 exe hash 与源 ico 一致，开始菜单/桌面快捷方式 `IconLocation` 均为 `%LOCALAPPDATA%\CCSwitchMulti\cc-switch.exe,0`。刷新 Explorer 图标缓存并重启安装版后，任务栏截图确认图标为暗色圆角底的新品牌图。

## 2026-06-29 Codex MultiRouter 502 Official Route Diagnosis

- 用户截图里 `OpenAI Multi-Model Router` 在 `06/29 16:07` 连续出现 5 条 502，但同一时间段 `codex-router.log` 证明每次都已命中 `route_id=openai-official`，`effective_provider=codex-openai-router::route::openai-official`，`effective_endpoint=/responses`，`upstream_url=https://chatgpt.com/backend-api/codex/responses`，`responses_to_chat=false`，`auth_strategy=CodexOAuth`。因此这不是 route miss、不是官方 GPT 被错误转 Chat、也不是模型重名映射错误。
- 502 的直接原因是本机转发到官方 Codex Responses 上游时连接失败：`upstream_send_error ... elapsed_ms=3121/3128/4127 error=请求转发失败:_连接失败:_error_sending_request_for_url_(https://chatgpt.com/backend-api/codex/responses)`。数据库对应 5 条 `status_code=502`，随后 `16:07:20` 同一 session/model/route 立刻成功 200，说明是间歇性出站连接问题。
- 网络边界证据：当场 `Resolve-DnsName chatgpt.com` 返回 `198.18.0.6`（常见代理/TUN fake-ip 网段），`Test-NetConnection chatgpt.com -Port 443` 当前成功；CCSwitchMulti 日志显示 `GlobalProxy Initialized: direct connection`，WinHTTP 也是 direct。也就是说 CCSM 没有显式走全局代理，而是依赖系统 TUN/fake-ip 接管；502 更像本机代理/TUN/VPN 或官方链路短时抖动，不是 CCSM MultiRouter 配置本身。
- 修复一个会误导排障的产品问题：失败路径历史上会把 usage/error 落库到外层 router provider，导致 UI 只显示 `OpenAI Multi-Model Router` 502。`src-tauri/src/proxy/handlers.rs` 现在在 forward error 落库前复用 Codex route 解析和 target materialize 逻辑，把失败归因修正到真实 route/effective provider；未来同类失败应显示 `codex-openai-router::route::openai-official` 等 route 身份。

## 2026-06-29 CCSwitchMulti v3.16.4-4wizard Wizard Prerelease

- `v3.16.4-4wizard` 已作为 BigStrongSun/ccswitchmulti 的 GitHub 预发布版发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-4wizard`。这是 `codex/multirouter-wizard` 分支的向导试用包，`isPrerelease=true`、`isDraft=false`，不是正式无向导 release 线。
- 本地 release pipeline 成功导出到 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，构建 metadata 指向提交 `ea455656521b8e53cbb448788ecb679f0b29e0b5`，版本面为 `3.16.4-4wizard`。本地 staging 目录为 `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.4-4wizard-assets`。
- GitHub release 上传 10 个资产：`CCSwitchMulti_3.16.4-4wizard_x64-setup.exe`、安装包 `.sig`、`CCSwitchMulti_3.16.4-4wizard_x64-portable.zip`、`CCSwitchMulti_3.16.4-4wizard_x64.exe`、`latest.json`、`SHA256SUMS.txt`、`linux-build-note.md`、`macos-build-note.md`、`RELEASE-METADATA.md`、`v3.16.4-4wizard-zh.md`。Linux/macOS 仍只上传平台构建说明，Windows 本地未生成正式二进制。
- 发布后验证：`latest.json` 版本为 `3.16.4-4wizard`；raw exe 的 `FileVersion/ProductVersion` 为 `3.16.4-4wizard`；GitHub asset digest 与本地主资产 SHA256 对齐，安装包为 `85825ada0fb485cc8c533e972c0cfce2717f87e9f3d5d60ef86aefc90c271c67`，portable zip 为 `5bcb76ef458f4bde734f4759fad79c69efee0a702a3cfabb044731f5285f5144`，raw exe 为 `61ee6f96129c512c88805faed1bac7bb42b54516dbf9933f2c3b9e123ecf05db`。
- 注意：`gh release view` 的 `targetCommitish` 可能显示 `main`，但 annotated tag `v3.16.4-4wizard` 本身的 tag object 指向 `ea455656521b8e53cbb448788ecb679f0b29e0b5`。在 PowerShell 里查询 peeled tag 必须给 `v3.16.4-4wizard^{}` 加引号，否则 `^`/`{}` 可能被 shell 或宿主参数解析干扰，出现误导输出。
- 发布时仍保留未跟踪文件 `docs/release-notes/v3.16.4-4-zh.md` 和 `scripts/logs/`：前者是无向导 release note，不能混入 wizard 试用 release；后者是本地日志/构建产物，不应随 release memory commit 提交。

## 2026-06-29 Windows Taskbar Icon Compatibility Fix

- 用户截图确认任务栏图标问题更像 Windows shell 兼容性/缓存问题，不是单纯 1024 源素材低清。根因排查：旧 `src-tauri/icons/icon.ico` 虽含 `16/24/32/48/64/128/256` 全尺寸，但所有 frame 都是 PNG-in-ICO；部分 Windows 任务栏、Explorer 缓存或快捷方式场景会对 PNG 小帧抽取/缩放异常，出现发糊或旧缓存视觉。
- 新增 `scripts/generate-windows-icons.py` 作为可复现图标生成脚本：`16/24/32/48/64` 使用专门为任务栏简化的高对比小图，并写入传统 32-bit DIB ICO frame；`128/256` 仍使用品牌 1024 图压缩为 PNG frame；同时更新 Tauri PNG、Windows Square logo 和应用内 `src/assets/icons/app-icon.png`。
- 验证点：解析 `src-tauri/icons/icon.ico` 时 `16/24/32/48/64` 的 frame 签名应为 `28 00 00 00...`（DIB），`128/256` 为 PNG；不要再回退到所有 frame 都是 PNG 的 ICO。实际安装后仍需确认快捷方式 `IconLocation` 为 `%LOCALAPPDATA%\CCSwitchMulti\cc-switch.exe,0`，并在 Windows 图标缓存较旧时重启 Explorer 或清理 icon cache。

## 2026-06-29 CCSwitchMulti v3.16.4-4 No-Wizard Release Boundary

- 用户确认 v3.16.4-3 不应作为正式无向导版发布，因为 tag `v3.16.4-3` 指向的 `1534a0e45dc17acd4b792de484f1c6b724cb7e18` 来自 `codex/multirouter-wizard`，仍包含 `CodexMultiRouterWizard`、`codexMultiRouterWizard` helper、ProviderList 的“配置多路模型”入口和向导测试。
- 正确修正策略不是删除或破坏 `codex/multirouter-wizard` 分支里的向导功能；wizard 分支要保留用于继续试用/迭代。正式无向导 release 应从不含向导的基线/分支切出，只合入已确认正式要带的 bugfix、provider 拆分、Responses-Lite fallback、子 Agent 用量统计、Windows 图标安装链路和 spawn_agent 模型可见性修复。
- 以后用户要求“发布当前版本”但近期分支名或 commit 含 `wizard/trial/easy` 时，必须先确认是否是试用向导包还是正式无向导包；不要直接从 `codex/multirouter-wizard` HEAD 打正式 release，也不要为了正式 release 在 wizard 分支上删除向导代码。

## 2026-06-29 CCSwitchMulti v3.16.4-3 Formal Release

- 2026-06-29 继续完善 MultiRouter 向导和单独 Codex provider 的协议探测：真实连通性测试必须先弹确认框，明确会向上游发送 `/v1/responses` 与 `/v1/chat/completions` 请求，可能消耗额度/流量/触发限流，输出上限为 1024 而不是 1。判断策略改为双协议结果矩阵：两者都通或仅 Responses 通时优先 Responses，仅 Chat 通时切到 Chat Completions，两者都不通时不能说成“不支持 Responses”，应提示 API Key、Base URL、模型权限、额度、网络或上游故障等更宽的失败边界。
- 2026-06-29 进一步明确：`/v1/responses` 探测通过不等于完整 Codex 功能正常。它只证明 Base URL/API Key/模型名/最小非流式 Responses 请求可用；完整功能还要看真实 Codex 会话里的 SSE/流式输出、工具调用、reasoning/上下文、多模态、限流稳定性、MultiRouter route 命中和历史修复后的 Desktop 状态。UI 文案必须称为“基础协议测试/基础请求可用”，不能把它包装成全功能验收。
- 2026-06-29 缓存命中调研结论：不要把“给 probe 加几个 Codex header”当成缓存验证。OpenAI/Codex prompt cache 主要依赖长前缀精确匹配、`prompt_cache_key` 路由、可选 `prompt_cache_retention`、body 前缀稳定和 `usage.prompt_tokens_details.cached_tokens`；DeepSeek Context Caching 默认自动启用，依赖后续请求完整复用已持久化的前缀单元，usage 字段是 `prompt_cache_hit_tokens/prompt_cache_miss_tokens`；Z.AI/GLM Context Caching 同样自动识别重复 system prompt/历史/长文档，并在 `usage.prompt_tokens_details.cached_tokens` 显示命中。适配优先级应是：保持真实 Codex OAuth 路径原生 Responses 不被转 Chat；Responses->Chat 转换要稳定 messages/tools/system 前缀和 canonical JSON；只对明确支持 OpenAI cache 参数的 Chat 上游考虑透传 `prompt_cache_key/prompt_cache_retention`，不能对 DeepSeek/GLM 一刀切注入未知字段。
- 2026-06-29 深度调研后的缓存适配边界：CCSM 的缓存适配应做成 provider/route capability，而不是把“Responses 探测成功”或“Chat 探测成功”当缓存结论。官方 OpenAI/Codex OAuth 走原生 Responses 并保留真实 client-provided session identity；OpenAI-compatible Chat 只有在明确声明支持 OpenAI prompt cache 参数时才透传 `prompt_cache_key/prompt_cache_retention`；DeepSeek/Z.AI/GLM 默认走自动前缀缓存，不注入 OpenAI 私有 cache 参数；Qwen/DashScope 兼具 Responses、隐式缓存和 `cache_control` 显式缓存，需按协议能力声明而非只按 provider 名称处理。当前代码缺口主要是：MultiRouter `CodexRoutingCapabilities` 无 cache capability 层；DeepSeek `prompt_cache_hit_tokens/prompt_cache_miss_tokens` 未统一映射到 `cache_read_tokens/input_tokens`；`openai_chat` 转换没有 cache capability gate；向导应展示“缓存兼容性/真实 usage 证据”，不要把基础连通性测试说成缓存命中验证。
- 2026-06-29 已实现缓存适配第一阶段：`ProviderMeta`/route capabilities 新增 `codexCache` capability 和 `promptCacheRetention`；MultiRouter route 物化时会把 `capabilities.codexCache` 带入运行时 meta，Codex Responses→Chat 转换只在 `cacheMode=openai_prompt_cache` 或显式 supports 时透传 `prompt_cache_key/prompt_cache_retention`，DeepSeek/GLM/Qwen 自动缓存路由不会收到 OpenAI 私有 cache 参数；Claude→Responses 增加显式 `promptCacheRetention` 支持，但 Codex OAuth 反代路径会移除 retention 以避免未确认参数触发 400；usage parser 现在能把 DeepSeek `prompt_cache_hit_tokens` 和 Qwen/DashScope details 里的 `cache_creation_input_tokens` 记入缓存统计。向导 route 预览页会显示每条 route 的缓存策略摘要，提示真实缓存命中看 usage 而不是基础连通性测试。
- 2026-06-29 修订 MultiRouter 向导收尾状态机边界：状态页进入“配置成功 -> 历史修复”不能只看任意最新 Codex proxy log 为 2xx/3xx；必须看到当前选中的 MultiRouter plan 的 route 有成功转发证据。`StatusTab` 现在用当前方案聚合后的 `trafficRows.successCount > 0` 作为 `currentRouteForwardOk`，只有当前 provider、代理监听、Codex 接管、路由入口和当前方案 route 转发都成立时才触发 `onRuntimeReady`。回归测试覆盖“不相关 provider/model 的成功日志不能触发历史修复交接”。
- 2026-06-29 `codex/multirouter-wizard` 合并 `main` 时确认 `main` 已是当前分支祖先，`git merge main` 返回 Already up to date；本轮只把向导试用版本面统一改为 `3.16.4-4wizard`（`package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`）。该版本号用于区分 wizard 试用构建，不代表无向导正式发布线。
- 2026-06-29 检查遗漏 bugfix 时同步远端发现 `upstream/main` / `origin/main`（`farion1231/cc-switch` 原版）新增两个明确修复并已 cherry-pick 到 `codex/multirouter-wizard`：`d1f6c74b` 修复 usage_script 凭据覆盖只应保存显式不同值，避免 provider 主 API Key/Base URL 修改后用量查询仍走旧覆盖；`61d7ac01` 修复 Windows Hermes 配置目录解析，按 settings override、`HERMES_HOME`、平台默认 `%LOCALAPPDATA%\hermes` 顺序对齐 Hermes 自身行为。`d1f6c74b` 与当前分支 provider live-config 保护逻辑在 `ProviderService` 函数区冲突，解决方式是同时保留 usage credential normalize 和 `codex_provider_requires_local_proxy`。
- 2026-06-29 按用户要求准备 `easy` 后缀试用包：版本面从 `3.16.4-3` 临时同步为 `3.16.4-3-easy`，用于重新打包验证 MultiRouter 向导更宽窗口、首页说明简化和步骤不回跳修复。该版本是本地试用包命名，不代表新的正式 GitHub release。
- 2026-06-29 `3.16.4-3-easy` 本地 Windows 试用包已由 post-commit pipeline 成功导出到 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，metadata 指向 `codex/multirouter-wizard` 的 `50499816cd3fcfd3c80fcc28ec156d011a855480`。可试用文件包括 `windows/installer/CCSwitchMulti_3.16.4-3-easy_x64-setup.exe`、`windows/portable/CCSwitchMulti_3.16.4-3-easy_x64-portable.zip`、`windows/raw-exe/CCSwitchMulti_3.16.4-3-easy_x64.exe`；`SHA256SUMS.txt` 覆盖 easy 资产，raw exe `FileVersion/ProductVersion=3.16.4-3-easy` 已验证。
- 2026-06-29 修复 MultiRouter 向导继续试用反馈：向导遮罩层级为 `z-[120]`，普通 `AddProviderDialog` 的 `FullScreenPanel` 只有 `z-[60]`，所以在向导第 2 步点击“添加 Provider/添加模型源”会像没反应；现在 `FullScreenPanel` 支持传入层级，App 只在向导打开时把 Add Provider 面板提升到 `z-[140]`。配置核心参数页也改为三态显示：可自动获取模型、已有模型目录可继续、需补全配置；创建模型源和配置页的 provider 卡片区有独立滚动高度，避免必须拉高整个窗口才能看到后续 provider。
- 2026-06-29 修复 MultiRouter 向导重名策略和官方协议推断：旧 `aliasModelName()` 取 `provider.id` 第一段，自动 ID 会生成 `3ecd52c8-gpt-*` 这类不可读前缀；现在第三方重名模型统一用 `模型名-provider展示名` 后缀，官方 OpenAI/订阅源保留原名，多个第三方互冲突时各自加 provider 名后缀并保留 `upstreamModel`。官方 OpenAI GPT/O 模型源即使旧 metadata 写 `openai_chat`，向导也生成 `openai_responses` route；OpenAI-compatible 中转不能仅因名字包含 openai 被当官方源。
- 2026-06-29 继续修正向导协议来源：旧 provider 可能因为历史保存、复制、混合协议拆分或第三方预设把 `meta.apiFormat/settingsConfig.apiFormat` 留成 `openai_chat`；向导不能把这个字段当最终真相。现在保存和预览前会把用户显式 `/v1/responses` 连通性测试结果应用到草稿，实测通过的 provider 自动提升为 `openai_responses`，失败/警告/跳过才保留旧 chat 或默认策略，避免一边探测通过一边仍生成 chat route。
- `v3.16.4-3` 已作为 BigStrongSun/ccswitchmulti 的 GitHub formal release 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-3`。Release 为非 draft、`prerelease=false`，发布时间为 `2026-06-29T02:02:35Z`，annotated tag `v3.16.4-3` 的 peeled commit 为 `1534a0e45dc17acd4b792de484f1c6b724cb7e18`（`chore(release): prepare CCSwitchMulti v3.16.4-3`）。
- 本地 Windows release pipeline 成功，导出目录为 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，完成时间 `2026-06-29 09:56:35 +08:00`。`latest.json` 版本为 `3.16.4-3`，updater URL 指向 `https://github.com/BigStrongSun/ccswitchmulti/releases/download/v3.16.4-3/CCSwitchMulti_3.16.4-3_x64-setup.exe`；raw exe `CCSwitchMulti_3.16.4-3_x64.exe` 的 FileVersion/ProductVersion 均验证为 `3.16.4-3`。
- 本次 release 上传 9 个资产：`CCSwitchMulti_3.16.4-3_x64-setup.exe`、安装包 `.sig`、`CCSwitchMulti_3.16.4-3_x64-portable.zip`、`CCSwitchMulti_3.16.4-3_x64.exe`、`latest.json`、`SHA256SUMS.txt`、`linux-build-note.md`、`macos-build-note.md`、中文 release note `v3.16.4-3-zh.md`。Linux/macOS 正式二进制仍未在 Windows 本地生成，只提供平台构建说明。
- 发布前验证覆盖：`pnpm typecheck`；`pnpm vitest run tests/components/CodexFormFields.test.tsx tests/components/AddProviderDialog.test.tsx tests/components/ProviderList.test.tsx tests/components/CodexMultiRouterWizard.test.tsx tests/lib/codexMultiRouterWizard.test.ts`；`cargo fmt --manifest-path src-tauri\Cargo.toml --check`；`cargo test --manifest-path src-tauri\Cargo.toml codex_model_catalog --lib`；`cargo test --manifest-path src-tauri\Cargo.toml codex_subagent_usage_stats --lib`；`cargo test --manifest-path src-tauri\Cargo.toml responses_lite --lib`；`git diff --check`。
- 发布过程中曾有一次手动 pipeline 与 post-commit pipeline 重叠，导致 Tauri artifact lock 等待；最终采用 post-commit pipeline 导出的 `3.16.4-3` 资产发布，并停止了冗余 `tmpC017` Tauri build 进程。以后 release 前如果已经触发 post-commit pipeline，先等 `scripts/logs/post-commit-release.log` 完成，避免再手动启动第二条 `tauri build`。

## 2026-06-28 Codex MultiRouter Wizard Implementation

- 2026-06-29 修复 MultiRouter 向导试用反馈的三处 UI/流程问题：遮罩窗口从 `max-w-5xl` 放宽为接近 1280px 的 `96vw` 宽度，内容区高度提高到 `82vh`，左侧步骤列加宽，避免默认窗口下 provider/路由组件挤压；第一页文案改为先说明“接入模型源、读取模型并处理重名、生成分流规则、启用并修复历史记录”四件用户任务，`127.0.0.1:15721` 等技术细节降级为备注。
- 这次“点到第 2 步又跳回第 1 步”的真实根因是 `CodexMultiRouterWizard` 在 `open/providers/existingPlan` effect 中每次 props identity 变化都会重新 `dispatchFlow({ type: "INIT" })`。`App.tsx` 原来 inline 传 `Object.values(providers)`，任意父级 rerender 都可能生成新数组，导致向导被重置。修复方式是向导内用 `initializedOpenRef` 保证每次打开只初始化一次，另设 provider 同步 effect 只追加/移除打开期间新建或删除的普通 Codex provider，不再派发 `INIT`；`App.tsx` 同时 memoize `codexWizardProviders` 降低无意义 props 变化。
- 2026-06-29 已为上述 UI/流程修复导出新的本地试用包 `3.16.4-3`：导出目录仍是 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，`RELEASE-METADATA.md` 指向 `codex/multirouter-wizard` 的 `1534a0e45dc17acd4b792de484f1c6b724cb7e18`，可试用文件包括 `windows/installer/CCSwitchMulti_3.16.4-3_x64-setup.exe`、`windows/portable/CCSwitchMulti_3.16.4-3_x64-portable.zip`、`windows/raw-exe/CCSwitchMulti_3.16.4-3_x64.exe`。手动触发的 pipeline 与 post-commit pipeline 并发时返回过一次 `tauri build failed with exit code -1`，但 post-commit pipeline 随后完成导出；最终 `SHA256SUMS.txt` 和 raw exe `FileVersion/ProductVersion=3.16.4-3` 已验证。
- 分支 `codex/multirouter-wizard` 新增 Codex 首页底部居中的 `配置多路模型` 入口，落点是 `ProviderList` 的 Codex 专属 CTA；空 Provider 列表也会显示该入口，右上角原有 MultiRouter 工作台图标入口保持不变。
- 新增遮罩式 `CodexMultiRouterWizard`：Portal 到 `document.body`，黑色 fixed overlay，按教程顺序引导理解本地 `15721` MultiRouter、创建模型源、检查 API Key/Base URL/API 格式、自动获取 `/models`、处理重名模型、生成按 provider 分组的 route、保存发布、显式启用并打开工作台 `test` 页。
- 2026-06-28 追加状态机改造：`CodexMultiRouterWizard` 不再只是 `stepIndex` 线性向导，而是由 `wizardFlowReducer` 显式维护 `opened/needSources/reviewProviderConfig/configIncomplete/readyToFetchModels/fetchingModels/modelFetchPartial/modelsFetched/collisionReviewRequired/routePreview/savingPlan/saveFailed/published/enablePrompt/enabling/enableFailed/enabled/completed/dismissed` 等状态。左侧步骤点击会映射到对应业务状态；下一步按钮会做 gate，例如无模型源停在 `needSources`，配置缺口进入 `configIncomplete`，保存失败进入 `saveFailed` 并展示错误。
- 状态机辅助数据落在 `src/lib/codexMultiRouterWizard.ts`：`getWizardConfigIssues()` 判断缺 Base URL/API Key 且没有可用 modelCatalog 的 provider；`collectWizardModelNameCollisions()` 收集同一 upstreamModel 被多个 provider 暴露的冲突，供向导进入 `collisionReviewRequired` 并提示别名策略。模型刷新现在复用 `mergeFetchedModelsIntoWizardProvider()`，保留已有手写字段。
- 新增 `src/lib/codexMultiRouterWizard.ts` 作为可单测数据层：普通 Codex provider 才作为模型源，MultiRouter provider 通过 `settingsConfig.codexRouting` 识别并排除；官方/OAuth 源没有普通 `/models` 时使用保守官方 catalog 兜底；第三方/中转站与官方同名模型会生成可见别名并保留 `upstreamModel` 指向真实上游模型。
- 向导保存策略：草稿留在 React state；只有点击“保存并发布”才调用 `providersApi.add/update` 写入带 `codexRouting` 和 `modelCatalog` 的 MultiRouter provider；不会静默切换当前 Codex provider，完成页“启用这个多路路由”复用 App 里的 `switchProvider` 路径，让既有 Codex 本地接管、PROXY_MANAGED、OAuth 保留逻辑继续生效。
- 路由生成策略：每个模型源一条 route，使用 `targetProviderId` + `auth.source="provider_config"`，不复制第三方 API Key/Base URL，不写 `requires_openai_auth`；默认按 provider/model 文本推断 `gpt`/`o`、`deepseek`、`qwen` 等前缀。
- 验证：`pnpm vitest run tests/components/ProviderList.test.tsx src/components/codex/CodexRouterWorkspacePage.test.ts tests/components/CodexFormFields.test.tsx tests/components/ProviderForm.codexCatalog.test.ts tests/components/CodexMultiRouterWizard.test.tsx tests/lib/codexMultiRouterWizard.test.ts` 通过；`pnpm typecheck` 通过；`cargo fmt --manifest-path src-tauri/Cargo.toml --check` 通过；`cargo test --manifest-path src-tauri/Cargo.toml codex_model_catalog --lib`、`model_fetch --lib`、`switching_codex_router_provider_auto_enables_dedicated_local_takeover --lib` 均通过。`pnpm format:check` 当前失败在两个未参与本次改动的既有文件 `src/components/codex/CodexRouterWorkspacePage.tsx` 和 `src/components/providers/forms/CodexFormFields.tsx`，本次未扩大 diff 去格式化无关大文件。
- 状态机改造验证：`pnpm vitest run tests/components/ProviderList.test.tsx src/components/codex/CodexRouterWorkspacePage.test.ts tests/components/CodexFormFields.test.tsx tests/components/ProviderForm.codexCatalog.test.ts tests/components/CodexMultiRouterWizard.test.tsx tests/lib/codexMultiRouterWizard.test.ts` 通过，48 tests passed；`pnpm typecheck` 通过；`git diff --check` 针对本次状态机改动文件通过。
- 2026-06-29 补齐向导每一步“异常/可继续”规则和真实 Responses 连通性探测：`CodexMultiRouterWizard` 的每个步骤卡片会展示本步骤可能失败的边界和继续条件；`fetchModels` 步骤新增用户显式点击的“测试 /v1/responses 连通性”，对每个普通 Codex provider 的可见模型发送最小 `input="ping"`、`max_output_tokens=1`、`stream=false` 请求，避免在自动刷新模型时静默消耗额度。
- 连通性状态机新增 `probingConnectivity/connectivityPassed/connectivityPartial/connectivityFailed`。`openai_responses` provider 的 `/v1/responses` 探测失败是阻塞项，不能保存发布；`openai_chat` provider 的直接 Responses 失败只是 warning，因为运行时 MultiRouter 会转到 `/chat/completions`；缺 Base URL/API Key 且已有 `modelCatalog` 时允许继续但标为 skipped，缺配置且没有目录时阻塞。
- 后端命令 `probe_codex_responses_for_config` 在 `src-tauri/src/commands/model_fetch.rs`，只做显式探测不缓存结果。URL 生成会把 provider 根地址、`/v1`、完整 `/v1/chat/completions` 或直接 `/v1/responses` 都收敛到 `/v1/responses`；HTTP 错误、网络错误和超时都结构化返回 `ok/status/url/model/detail`，错误体截断到 512 字符，避免 UI/日志被上游 HTML 或长 JSON 淹没。
- 本轮为满足全局格式检查，额外对既有 `src/components/codex/CodexRouterWorkspacePage.tsx` 做了纯 Prettier 格式化，不改业务逻辑；`pnpm format:check` 现在通过。
- 2026-06-29 继续补强异常可见性：`CodexMultiRouterWizard` 新增向导级 `wizardIssues` 列表，所有异步 catch 不能只发 toast，必须写入 UI 问题面板并标明 `错误/警告`、provider、异常详情和 `可继续/需处理后继续`。当前覆盖 `/models` 单 provider 失败、整体刷新中断、`/v1/responses` IPC/命令异常、保存失败、启用失败，以及用户尝试越过阻塞连通性结果的场景。
- 2026-06-29 补齐 MultiRouter 向导发布后的自动收尾：用户点击“启用这个多路路由”后，App 会先启动 CCSwitchMulti 本地代理，再打开 Codex live 接管，随后切换当前 Codex provider 到该 MultiRouter 方案并打开工作台 `status` 页。状态页不会只因配置态全绿就提示成功，必须同时看到最近一次 Codex 代理转发为 2xx/3xx，确保“当前链路、监听、Codex 接管、路由入口、最近转发”都成功后才 toast “配置成功”并跳到 Codex 历史修复页。
- 2026-06-29 完整引导交接细化：`CodexMultiRouterWizard` 启用成功后必须自动关闭遮罩，让用户看到 App 已打开的 MultiRouter `status` 页；toast 明确要求去 Codex 发送一次请求，等待当前链路、监听、Codex 接管、路由入口和最近转发都成功。向导里的“打开工作台”也改为打开 `status` 页，不再跳 `test` 页，避免绕开五项状态验证。
- 2026-06-29 入口选择规则：Codex 首页底部 `配置多路模型` 不再直接打开向导，而是每次先弹出入口选择面板；用户可以随时关闭退出，也可以选择“开始引导配置”进入遮罩式向导，或选择“直接打开工作台”进入 MultiRouter `status` 页。这个选择不受 dismissed localStorage 影响，确保用户再次点击入口仍可决定是否开启引导。
- 历史修复页新增向导收尾入口：`SessionManagerPage` 通过一次性 `initialCodexHistoryRepair` 自动打开 `CodexHistoryRepairPanel` 并消费标记；自动跳转进入时面板顶部会显示历史修复点击顺序：加载历史、预览修复、确认写入、完整重启 Codex、打开 GitHub 仓库点 Star。真实应用历史修复成功后回调 App，提示用户完整重启 Codex，然后先请求用户给 CCSwitchMulti GitHub 仓库点 Star，再用默认浏览器打开 `https://github.com/BigStrongSun/ccswitchmulti`。如果后续引导回调失败，只报“历史修复已完成，但后续引导失败”，不能把修复本身标记为失败。
- 2026-06-29 已为 MultiRouter 向导试用打本地 Windows 包：运行 `scripts/local-release-pipeline.ps1 -Reason manual-multirouter-wizard-test` 成功，导出目录 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，metadata 指向分支 `codex/multirouter-wizard`、提交 `214bc5b4650e20d3de7cc13a3ff113cda63b00c4`、版本 `3.16.4-2`。可试用文件包括 `windows/installer/CCSwitchMulti_3.16.4-2_x64-setup.exe`、`windows/portable/CCSwitchMulti_3.16.4-2_x64-portable.zip`、`windows/raw-exe/CCSwitchMulti_3.16.4-2_x64.exe`；这是本地试用包，不是正式 release bump。

## 2026-06-28 Mixed Relay Responses Capability Boundary

- 当前 MultiRouter 对 Codex `/responses` 的上游协议选择是 route/effective-provider 级配置判定，不是模型级在线能力探测。运行时入口是 `src-tauri/src/proxy/providers/codex.rs::explain_codex_responses_upstream_protocol`，优先看 managed `codex_oauth`、`meta.apiFormat`、`settings_config.apiFormat/api_format`、已知 chat-only base_url、`config.toml wire_api`，最后默认原生 Responses。
- 对“同一个中转里既有 GPT/Responses 模型，也有 Qwen/DeepSeek 等 Chat-only 模型”的正确现有用法是拆成多条 route：GPT/Responses 模型 route 写 `upstream.apiFormat=openai_responses`，Chat-only 模型 route 写 `upstream.apiFormat=openai_chat`。如果 route 引用 `targetProviderId`，`materialize_codex_routed_provider_from_target` 会继承目标 provider 的 base_url/auth/apiFormat；因此同一个目标 provider 不能天然表达“部分模型 responses、部分模型 chat”，除非拆成两个 provider 或使用内联 route 覆盖协议。
- 目前 `/models` 刷新只读取模型 id、owned_by、context_window 等元数据并写回 `modelCatalog`，`CodexCatalogModel` 和 `CodexRoutingCapabilities` 只有图片/文本/推理相关能力，没有 `supportsResponses` / per-model `apiFormat` 字段。状态页“协议探测”读取配置判定和 `codex-router.log` 最近真实请求的 `effective_endpoint/responses_to_chat`，不会主动请求远端 `/v1/responses`，所以不会自动发现某个模型不支持 Responses。
- 若后续实现在线探测，应做成显式手动/批量按钮而不是自动刷新时静默执行：对每个候选模型发最小 `/v1/responses` 探测请求，识别 404/405/400 unsupported endpoint/model 等结果并缓存到 provider `modelCatalog.models[].supportsResponses` 或 `apiFormat`；探测会消耗额度、可能触发供应商限流，也可能误判“模型不支持”与“账号无权限/渠道暂不可用”，因此结果应带时间戳、错误摘要和手动覆盖入口。
- 2026-06-28 普通 Codex provider 新增表单“获取模型”路径加入保守拆分提示：当 `/models` 同时返回 GPT-like（如 `gpt-*`、OpenAI namespace 下的 gpt、o 系列）和非 GPT-like 模型，且当前表单还没有用户手写 route 时，`CodexFormFields` 只弹出“检测到混合协议模型”确认框，不会静默写入 routing。用户确认后只记录提交意图并打开本地接管；真正点击新增时由 `AddProviderDialog` 生成两个独立 provider：`<providerName>-responses` 写 `meta.apiFormat=openai_responses` 且只保留 Responses 模型目录，`<providerName>-chat` 写 `meta.apiFormat=openai_chat` 且只保留 Chat 模型目录；用户取消时只保留已获取的模型列表，routing、provider 拆分意图和 apiFormat 都不变。编辑已有 provider 时不启用自动拆 provider，避免“编辑 A 生成 B/C”的危险行为。

## 2026-06-28 Responses-Lite Header Retry Fallback Policy

- 用户指出“第三方一律剥 `x-openai-internal-codex-responses-lite`”仍然过宽，因为未来第三方上游可能支持 Responses-Lite，提前砍掉 header 可能影响它们自己的 Lite 路径、prompt cache 或其它能力协商。策略已改成 optimistic pass-through：默认保留该 header 发给上游，只有上游明确返回 `This model is not supported when using X-OpenAI-Internal-Codex-Responses-Lite.` 这类错误时，才剥离 header 并对同一个 provider 重发一次。
- 实现落点仍在 `src-tauri/src/proxy/forwarder.rs`。发送前不再调用静态 strip helper；错误响应体读取并解压后调用 `should_retry_without_codex_responses_lite_header()` 判断，条件是 `AppType::Codex`、请求里确实有 Lite header、状态码为 `400/404/422/501` 之一、错误体包含精确 Lite 不支持文本。命中后记录 `upstream_retry_without_responses_lite`，移除该 header 后只重试一次；普通 400、非 Codex app、无 header 或错误体不匹配都不重试。
- 2026-06-28 进一步改为带过期时间的短期能力负缓存，避免同一上游/模型在连续请求里每次都先失败一次。缓存是内存态，TTL 为 24 小时，key 按 effective provider id、上游 URL 的 scheme/host/port/path、实际请求模型隔离，并忽略 query 以避免敏感参数进入缓存 key。命中缓存时直接去掉 Lite header 发送并记录 `responses_lite_fallback_cache_hit`；过期后自动删除并重新带 header 探测，防止未来第三方上游支持 Lite 后仍被永久去头。
- 验证通过：`cargo fmt --manifest-path src-tauri\Cargo.toml --check`；`cargo test --manifest-path src-tauri\Cargo.toml responses_lite --lib`（6 passed）；`cargo test --manifest-path src-tauri\Cargo.toml codex_responses_lite_error_triggers_retry_without_header --lib`。

## 2026-06-28 Responses-Lite Header Source And Proxy Failure Mechanism

- OpenAI Codex 源码确认 `x-openai-internal-codex-responses-lite` 不是普通透传 header，而是由模型元数据 `ModelInfo.use_responses_lite` 驱动的官方内部协商信号。`codex-rs/protocol/src/openai_models.rs` 定义 `use_responses_lite: bool`；`codex-rs/core/src/client.rs::add_responses_lite_header()` 在该值为 true 时给 HTTP Responses 请求加入 `x-openai-internal-codex-responses-lite: true`；WebSocket 路径则在 `build_ws_client_metadata()` 中写入 `ws_request_header_x_openai_internal_codex_responses_lite=true`。
- Lite 模式还会改变请求结构，不只是多一个 header：`build_responses_request()` 用 `prompt.get_formatted_input_for_request(model_info.use_responses_lite)`；Lite 为 true 时会去掉图片 detail、把 tools 放进 `AdditionalTools`/instructions 前缀、关闭 `parallel_tool_calls`，并让部分 tool planning 走 Lite 分支。说明服务端会按这个信号选择不同 Responses 处理路径。
- 中转遇到问题的根因是“协议能力错配”：Codex 官方客户端/后端之间的私有能力信号被 CCSwitchMulti 或其它代理原样转发给第三方 OpenAI-compatible 上游，或者转发给当时尚未支持该模型 Lite 路径的官方后端分支。上游看到 header 后按 Lite 路径校验模型，若该模型/账号/区域/后端版本不支持 Lite，就返回 `This model is not supported when using X-OpenAI-Internal-Codex-Responses-Lite.`。最新策略不是预先剥离，而是默认透传、命中特定 Lite 不支持错误后剥头重试一次。

## 2026-06-28 Responses-Lite Header Strip Policy Narrowed

- 上游作者关闭 `#4727` 后重新评估，原先 `should_strip_codex_private_header_for_upstream(_url, name)` 只看 header 名、无条件剥 `x-openai-internal-codex-responses-lite` 的策略过宽。这个 header 对第三方 OpenAI-compatible / MultiRouter 目标确实是官方私有信号，不应透传；但托管 ChatGPT Codex OAuth 目标属于官方协议路径，应该保留给官方后端自行协商，避免改变 Responses-Lite / prompt cache / 官方内部能力分支。
- 该静态剥离策略后来被进一步收窄为 fallback 重试策略：默认保留 header，只有上游明确返回 Lite 不支持错误时剥头重试一次。不要再恢复“第三方 Codex/OpenAI-compatible 上游发送前直接剥离”的口径。
- 这次验证时主工作区 `src-tauri/tauri.conf.json` 已有未归属脏改，新增 `bundle.windows.nsis.uninstallerIcon` 被当前 `tauri-build` 拒绝，导致主工作区 `cargo test --manifest-path src-tauri\Cargo.toml codex_responses_lite_header --lib` 卡在 build script。为不修改用户的 NSIS/icon 改动，使用临时 detached worktree `C:\Users\sunda\Documents\cc-switch-test-responses-lite` 套同一份 `forwarder.rs` 改动验证：`cargo fmt --manifest-path src-tauri\Cargo.toml --check` 通过；`cargo test --manifest-path src-tauri\Cargo.toml codex_responses_lite_header --lib` 通过 3 个用例：官方托管保留、第三方剥离、非 Codex app 保留。

## 2026-06-28 Windows Taskbar Icon Install Verification

- 本地 release pipeline 导出的 raw exe `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti\windows\raw-exe\CCSwitchMulti_3.16.4-2_x64.exe` 已经正确嵌入 `src-tauri/icons/icon.ico`；用 `System.Drawing.Icon.ExtractAssociatedIcon()` 抽取后和源 `icon.ico` 一致，都是新的白色云/青色底图标。
- 用户看到 Windows 任务栏仍是旧图标时，优先检查启动路径。开始菜单和桌面快捷方式默认指向安装目录 `%LOCALAPPDATA%\CCSwitchMulti\cc-switch.exe`，而不是导出目录 raw exe。若只运行 raw exe 或只生成导出产物，固定任务栏/开始菜单仍可能从旧安装目录或 Windows 图标缓存读取旧图标。
- 这次用 `CCSwitchMulti_3.16.4-2_x64-setup.exe /S` 静默安装后，`%LOCALAPPDATA%\CCSwitchMulti\cc-switch.exe` 被替换为 3.16.4-2，内嵌图标抽取结果也变成新图标；监听端口 `15721/15722` 由安装版 `cc-switch.exe` 接管。若任务栏视觉仍旧，剩余边界是 Windows Explorer / 任务栏固定项图标缓存，需要刷新快捷方式或重启 Explorer，而不是重新修 Tauri 图标配置。
- 进一步固化在 `src-tauri/tauri.conf.json` 的 `bundle.windows.nsis`：当前项目使用的 `tauri-build` 只接受 `installerIcon`，不能写 `uninstallerIcon`；安装包图标显式设置为 `icons/icon.ico`，并通过 `src-tauri/nsis/installer-hooks.nsh` 的 `NSIS_HOOK_POSTINSTALL` 重写已存在的开始菜单和桌面快捷方式，把 `IconLocation` 固定为安装目录里的 `cc-switch.exe,0`。验证脚本为 `scripts/verify-windows-install-icon.ps1`，用于比对源 ico、安装目录 exe 内嵌图标和快捷方式图标目标。

## 2026-06-28 MultiRouter spawn_agent Model Override Visibility Fix（2026-07-10 已被新版保留 schema 修复取代）

- 用户截图里 `spawn_agent` 工具提示“没有显式 model 选择字段”的根因不是提示词没写模型名，也不是单纯 catalog 前五候选排序问题；对照 `openai/codex` 最新源码确认，`multi_agent_v2` 的 `create_spawn_agent_tool_v2()` 在 `hide_spawn_agent_metadata=true` 时会调用 `hide_spawn_agent_metadata_options()`，直接从工具 schema 删除 `agent_type`、`model`、`reasoning_effort`、`service_tier`。新版 Codex 的 `MultiAgentV2Config::default()` 默认 `hide_spawn_agent_metadata=true`，所以只把 `qwen3.6` 写进 message 会继承父模型。
- 2026-07-10 更正：`hide_spawn_agent_metadata=false` 会让新版 GPT/Codex 把 `collaboration.spawn_agent` 判定为保留工具 schema 不匹配；当前正确边界是保持/写入 `hide_spawn_agent_metadata=true`，并通过 `~/.codex/agents/*.toml` custom agent role 文件固定子 Agent 的模型、provider 和 reasoning 配置。
- Codex 源码还确认 `spawn_agent_models_description()` 只展示 `ModelPreset.show_in_picker` 的前 5 个，而 `ModelPreset.show_in_picker` 来自 `ModelInfo.visibility == list`。因此 catalog 条目必须同时保留新版 `ModelInfo` snake_case 字段（`slug`、`visibility=list`、`supported_in_api=true`、`default_reasoning_level`、`supported_reasoning_levels`）和旧 renderer / 旧 direct preset 路径字段（`id`、`show_in_picker=true`、`hidden=false`、`defaultReasoningEffort`、`supportedReasoningEfforts`）。
- Provider inline `models` 也要同步补齐 `slug`、`description`、`visibility=list`、`show_in_picker=true`、`supported_in_api=true`、`default_reasoning_level`、`supported_reasoning_levels`，避免只写顶层 `model_catalog_json` 时某些 Desktop 热切路径看到不完整模型元数据。
- 回归测试落点：`cargo test --manifest-path src-tauri/Cargo.toml codex_model_catalog_projects_spawn_agent_model_info_fields --lib`、`cargo test --manifest-path src-tauri/Cargo.toml codex_multi_agent_v2_keeps_spawn_agent_reserved_schema_compatible --lib`、`cargo test --manifest-path src-tauri/Cargo.toml codex_model_catalog_ --lib`，并配合 `cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`git diff --check`。

## 2026-06-28 MultiRouter Subagent Usage Model Aggregation Fix

- “今日子 Agent 会话流量”全 0 的根因不是前端数值格式化，而是 Codex 子 Agent JSONL 的 `session_meta.payload.session_id` 在当前 Codex Desktop 中指向父线程 ID；子 Agent 自己的线程 ID 在 `state_5.sqlite.threads.id` 和 rollout 文件名后缀里。旧 `session_usage_codex.rs` 同步时把 `proxy_request_logs.session_id` 写成父线程，导致 `build_codex_subagent_usage_stats_from_history()` 用子 Agent id 做 `data_source='codex_session' AND session_id IN (...)` 时查不到当天用量。
- 修复边界分两层：后续同步在 `session_meta` 标记为 `source.subagent.thread_spawn` / `source.thread_spawn` 时，优先用 rollout 文件名里的 36 位线程 ID 作为 `session_id`；已有错归到父线程的历史/当天数据不迁移 DB，而是在子 Agent 统计页按子 Agent rollout JSONL 只读回退解析 `token_count`，恢复 request/token/model 聚合。
- 模型聚合不能依赖是否已有 token_count 命中。`modelStats.agentCount` 现在从子 Agent 的 `turn_context` / `token_count` primary model 归并，每个模型一行展示子 Agent 数、请求、Tokens、费用；即使某个模型的子 Agent 暂无用量，也要显示 agentCount，避免页面退化成几百个子 Agent 明细行。
- 前端 `CodexRouterWorkspacePage.tsx` 的子 Agent 会话流量区默认只保留模型聚合表和一行数据源摘要，不再默认渲染逐子 Agent 明细表。这样状态页回答“每个模型有多少子 Agent、消耗多少 token”，而不是“每个子 Agent 用了什么模型”。
- 回归测试落点：`cargo test --manifest-path src-tauri/Cargo.toml codex_subagent_usage_stats --lib`、`cargo test --manifest-path src-tauri/Cargo.toml test_codex_subagent_model_stats_counts_agents_without_usage --lib`、`cargo test --manifest-path src-tauri/Cargo.toml test_sync_codex_subagent_uses_rollout_thread_id --lib`，并配合 `cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`pnpm typecheck`、`git diff --check`。

## 2026-06-28 CCSwitchMulti v3.16.4-2 Formal Release

- `v3.16.4-2` 已作为 BigStrongSun/ccswitchmulti 的 GitHub 正式 release 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-2`。Release 为非 draft、`prerelease=false`，发布时间为 `2026-06-28T05:00:55Z`。本地 tag `v3.16.4-2` 为 annotated tag，tag 对象为 `cf874abd37e10f767971deea69e0178edfd0aa71`，解引用到版本提交 `d81abacdccb6915e31ebf829e50155ae95f64a37`（`chore(release): prepare CCSwitchMulti v3.16.4-2`）。
- 本次正式版覆盖 `v3.16.4-1` 之后的两个用户可见修复：`fa32a34c` 新增异常退出 / panic / 正常退出结构化日志与“打开日志目录”入口；`7ebd7354` 修复 Codex Desktop `x-openai-internal-codex-responses-lite` 内部 header 被转发到真实上游导致 gpt-5.5 等模型 HTTP 400 的问题。版本面统一更新为 `3.16.4-2`，并新增中文 release note `docs/release-notes/v3.16.4-2-zh.md`。
- Windows 本地 release pipeline 由 post-commit hook 启动并成功完成，导出目录为 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，完成时间 `2026-06-28 12:57:52 +08:00`。raw exe `CCSwitchMulti_3.16.4-2_x64.exe` 的 FileVersion/ProductVersion 均验证为 `3.16.4-2`，下载后的 `latest.json` 也验证为 `version=3.16.4-2` 且指向 `https://github.com/BigStrongSun/ccswitchmulti/releases/download/v3.16.4-2/CCSwitchMulti_3.16.4-2_x64-setup.exe`。
- 本次 release 上传 9 个平铺资产：`CCSwitchMulti_3.16.4-2_x64-setup.exe`、安装包 `.sig`、`CCSwitchMulti_3.16.4-2_x64-portable.zip`、`CCSwitchMulti_3.16.4-2_x64.exe`、`latest.json`、`SHA256SUMS.txt`、`linux-build-note.md`、`macos-build-note.md`、`v3.16.4-2-zh.md`。`SHA256SUMS.txt` 是从平铺 staging 目录 `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.4-2-assets` 重新生成的，GitHub asset digest 与本地 checksum 对应。Linux/macOS 正式二进制未在 Windows 本地构建，本次仍上传平台构建说明。
- 发布前验证通过：`pnpm typecheck`；`cargo fmt --manifest-path src-tauri\Cargo.toml --check`；`cargo test --manifest-path src-tauri\Cargo.toml codex_responses_lite_header --lib`；`cargo test --manifest-path src-tauri\Cargo.toml ordinary_headers_are_preserved_for_upstream --lib`；`cargo test --manifest-path src-tauri\Cargo.toml app_exit_monitor --lib`；`git diff --check`。发布后验证：`gh release view v3.16.4-2 --repo BigStrongSun/ccswitchmulti --json tagName,isDraft,isPrerelease,publishedAt,url,assets`、`gh api repos/BigStrongSun/ccswitchmulti/releases/latest`、`git show-ref --tags v3.16.4-2`、下载并解析 release `latest.json`。

## 2026-06-28 Codex Responses-Lite Header Upstream Strip

- `This model is not supported when using X-OpenAI-Internal-Codex-Responses-Lite` 的根因不是 MultiRouter 自身路由错误，而是 Codex Desktop 发给本地后端的内部协商头 `x-openai-internal-codex-responses-lite` 被 CC Switch / CCSwitchMulti 的 `forwarder.rs` 默认透传到了真实上游。OpenAI 在 2026-06-26 左右收紧 Lite 路径后，`gpt-5.5` 等模型会因此在 official ChatGPT Codex upstream 或第三方代理 upstream 返回 HTTP 400。
- 正确修复边界在转发层 header policy：`src-tauri/src/proxy/forwarder.rs` 构建 `ordered_headers` 时，在默认透传前调用 `should_strip_codex_private_header_for_upstream()`，无条件移除 `x-openai-internal-codex-responses-lite`。不要把它修成 UI 开关、catalog schema、模型映射或 MultiRouter route 规则；也不要粗暴移除 OAuth/session/account headers，否则会破坏 Codex 官方登录态、前缀缓存和 CCSwitchMulti 之前的 OAuth login-preservation 修复。
- 这次先在原版 `C:\Users\sunda\Documents\LLMservice\ccswitch official` 基于 `origin/main` 创建 `codex/strip-codex-responses-lite-header`，提交 `1e6a46b7 fix(proxy): strip Codex Responses-Lite header upstream`，并向 `farion1231/cc-switch` 提交 PR `#4727`，关联 issue `#4700`。随后把同一策略移植到 CCSwitchMulti `C:\Users\sunda\Documents\LLMservice\cc-switch` 的 `codex/merge-official-v3.16.4` 分支。
- 回归测试落点：`proxy::forwarder::tests::codex_responses_lite_header_is_stripped_for_official_upstream`、`codex_responses_lite_header_is_stripped_for_third_party_upstream`、`ordinary_headers_are_preserved_for_upstream`。验证命令优先跑 `cargo fmt --manifest-path src-tauri\Cargo.toml --check`、`cargo test --manifest-path src-tauri\Cargo.toml codex_responses_lite_header --lib`、`cargo test --manifest-path src-tauri\Cargo.toml ordinary_headers_are_preserved_for_upstream --lib`、`git diff --check`。

## 2026-06-28 CCSwitchMulti v3.16.4-1 Prerelease

- `v3.16.4-1` 已作为 BigStrongSun/ccswitchmulti 的 GitHub prerelease 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-1`。Release 为非 draft、`prerelease=true`，发布时间为 `2026-06-27T20:52:24Z`，target commit 为 `e0228d531d1a7086a808d706e6ecb2618de44f4c`（`docs(memory): record completed v3.16.4-1 merge`）。
- 本地 Windows release pipeline 成功，导出目录为 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，完成时间 `2026-06-28 04:50:26 +08:00`。raw exe `CCSwitchMulti_3.16.4-1_x64.exe` 的 FileVersion/ProductVersion 均验证为 `3.16.4-1`，`latest.json` 指向 `https://github.com/BigStrongSun/ccswitchmulti/releases/download/v3.16.4-1/CCSwitchMulti_3.16.4-1_x64-setup.exe`。
- 本次 prerelease 上传 8 个资产：`CCSwitchMulti_3.16.4-1_x64-setup.exe`、安装包 `.sig`、`CCSwitchMulti_3.16.4-1_x64-portable.zip`、`CCSwitchMulti_3.16.4-1_x64.exe`、`latest.json`、`SHA256SUMS.txt`、`linux-build-note.md`、`macos-build-note.md`。Linux/macOS 正式二进制未在 Windows 本地构建，后续需要 supplemental workflow 或对应平台构建补齐。
- 发布前后验证：`pnpm release:local` 运行了 `pnpm typecheck` 并完成 Tauri NSIS Windows x64 build；使用本地 `C:\Users\sunda\.ccswitchmulti\tauri-update.key` 生成 updater 签名；`gh release view v3.16.4-1 --repo BigStrongSun/ccswitchmulti --json tagName,targetCommitish,isDraft,isPrerelease,publishedAt,url,assets` 复核 release 状态和资产摘要；`SHA256SUMS.txt` 中 Windows 资产 hash 与 GitHub release asset digest 对应。

## 2026-06-28 CCSwitchMulti v3.16.4-1 Official Merge Completed

- `codex/merge-official-v3.16.4` 已完成官方 `farion1231/cc-switch` `v3.16.4` 跟进，版本面更新为 `3.16.4-1`。不要把它理解成直接 merge 官方 tag；这次按 `45555638..e50fc0eb` 的缺口逐个 cherry-pick / 手工合并，保留了 `cc-switch-multi`、`CCSwitchMulti`、`com.ccswitchmulti.desktop`、BigStrongSun updater、MultiRouter workspace、外部 OpenAI-compatible API、Codex history repair、WebDAV/S3 sync 和 fork release 脚本。
- 高风险合并点的最终边界：`codex_oauth_auth.rs` 采用官方共享 `crate::proxy::http_client::get()` 出站，但保留 Multi 的 `oauth_token_url` 测试注入和 refresh-token 轮换语义；`forwarder.rs` 合入 zstd/gzip/br/deflate 解压与 local proxy request overrides，同时保留 Multi 的 4 元组返回和 `codex_router_log`，上游错误摘要使用解压后的 body；`ProviderMeta` 同时保留 `min_output_tokens` 和官方 `local_proxy_request_overrides`。
- Codex 表单合并时必须记住：官方 `apiFormat` 已变成上游 wire format 选择，Multi 的本地路由/模型映射由 `takeoverEnabled` 独立控制。`CodexFormFields`/`ProviderForm` 已采用 `takeoverEnabled` / `codexTakeoverEnabled`，保留 MultiRouter catalog/routing、visible model 与 upstream model 分离；不要恢复旧的“apiFormat == chat 才显示路由”的耦合逻辑。
- 已合入的官方功能包括 ETok rename、Kimi 图标/赞助文案/auto compact、Volcengine Ark AK/SK usage、Skills UI 修复、Windows ARM64 release workflow、Usage live end time、JsonEditor dark mode、DB too-new recovery screen、local proxy request overrides、Copilot/Codex OAuth 全局代理 client、body 解压、Doubao Seed 2.1、Codex CN providers native Responses presets、SubRouter/OpenCode Go presets、v3.16.4 docs/release notes、Fable 5 banner removal 和 fork 版本 bump。
- 收尾测试修复：`tests/components/CodexFormFields.test.tsx` 的 test harness 需要传入 `takeoverEnabled`、`onTakeoverEnabledChange`、`localProxyHeadersOverride`、`onLocalProxyHeadersOverrideChange`、`localProxyBodyOverride`、`onLocalProxyBodyOverrideChange`。否则 `pnpm typecheck` 会报缺必填 props，Vitest 会在 `trim()` 处因 undefined 崩溃；这是测试壳没跟上组件契约，不是生产逻辑需要默认兜底。
- 本轮验证通过：`pnpm typecheck`；`vitest run tests/components/CodexFormFields.test.tsx tests/config/codexChatProviderPresets.test.ts tests/config/subrouterProviderPresets.test.ts tests/lib/requestOverrides.test.ts src/components/codex/CodexRouterWorkspacePage.test.ts`；`cargo fmt --manifest-path src-tauri/Cargo.toml --check`；`cargo test --manifest-path src-tauri/Cargo.toml local_proxy_ --lib`；`cargo test --manifest-path src-tauri/Cargo.toml content_encoding --lib`；`cargo test --manifest-path src-tauri/Cargo.toml token_request_ --lib`；`cargo test --manifest-path src-tauri/Cargo.toml get_status_does_not_refresh --lib`；`git diff --check`。广义 `pnpm test:unit -- ...` 早先因脚本展开跑到 `tests/integration/App.test.tsx` 出现过一次 timeout，目标测试收敛后未复现，若发布前做全量 CI 仍需关注该集成测试是否环境性超时。

## 2026-06-28 Official v3.16.4 Delta And CCSwitchMulti Merge Boundary

- Official `farion1231/cc-switch` `v3.16.4` was verified from GitHub release/tag: release published `2026-06-27T05:14:41Z`, tag `v3.16.4` points to `e50fc0eb281cf937251a1cb24a44e792d69029ac`. Local `git diff v3.16.3..v3.16.4 --stat` shows 57 commits and 138 files changed with `+9409/-1020`; the release note itself summarizes 53 commits / 126 files / `+8149/-1016`, so use git as the exact source for merge planning and release notes as product summary.
- Current CCSwitchMulti `main` is `23c43f59e124db15608f9192a89a2e6dd141434e` (`docs(memory): record v3.16.3-23 release`), version surfaces are `3.16.3-23`, and `git merge-base HEAD v3.16.4` is official commit `455556380b52c18d3d444a751a6c17de6d4ee5b0` (`Chat API: skip tool calls with missing function names`). That means CCSwitchMulti has already absorbed the official v3.16.4 path through `45555638`; do not re-merge earlier commits such as CODEX_SQLITE_HOME probing, cached tool-call restore, DeepSeek `thinking:disabled` effort stripping, settings scroll reset, models.dev pricing import, duplicate Codex `base_url` cleanup, or Add Provider search click fix.
- Do not merge the full official tag into CCSwitchMulti. `git merge-tree HEAD v3.16.4` reports direct conflicts in fork identity and high-divergence files including `README.md`, `package.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json`, `src-tauri/src/proxy/forwarder.rs`, `src-tauri/src/proxy/mod.rs`, `src-tauri/src/proxy/providers/codex_oauth_auth.rs`, `src/components/providers/forms/CodexFormFields.tsx`, `src/components/providers/forms/ProviderForm.tsx`, locale JSON files, and `tests/config/codexChatProviderPresets.test.ts`. A full tag merge also appears to delete many CCSwitchMulti-only modules when viewed as `HEAD..v3.16.4`.
- Fork identity must be preserved during any v3.16.4 merge: `cc-switch-multi` package name, `CCSwitchMulti` product name, `com.ccswitchmulti.desktop` identifier, BigStrongSun updater endpoints/signing, release/export scripts, supplemental Linux/macOS workflows, `codex-history-repairer`, MultiRouter workspace, external OpenAI-compatible API, WebDAV/S3 sync, Codex history repair UI/tooling, model fetch/catalog overlay behavior, and the Codex OAuth login-preservation fixes from `v3.16.3-23`.
- The still-missing official commits are `6ec86cff..e50fc0eb` after merge-base `45555638`: Homebrew docs cleanup; CTok to ETok rename; Kimi icon/prime-partner/order updates; Volcengine Ark AK/SK usage; Skills UI fixes; Kimi auto compact window; Windows ARM64 release support; live end time in usage range; JsonEditor dark mode; database-too-new recovery screen; local proxy request overrides; Copilot/Codex OAuth global proxy fix; zstd/gzip/br/deflate request and error body decompression; Doubao Seed 2.1 pricing/preset; Codex upstream format selector decoupling; unmanaged skill green dot; native Responses API presets for CN Codex providers; SubRouter and OpenCode Go presets; v3.16.4 docs/release notes; Fable 5 banner removal; and official version bump.
- Low-risk/high-value merge candidates for CCSwitchMulti are: `1a0e8c7a` zstd/body decompression, `524b9d98` Copilot/Codex OAuth requests using shared global proxy client, `9171ad75` usage live end time, `55abd182` JsonEditor dark mode, `f1328d89` unmanaged skill green dot, `2d478876` Claude MCP custom config path, `2781d40e` Skills/card UI fixes, `c4630b5c` Volcengine Ark usage query, `2e547c98`/`fdf538e5` Doubao Seed 2.1 pricing/preset, and provider pricing/preset additions that do not overwrite fork-specific catalog behavior.
- Medium/high-risk items need manual hunk porting, not blind cherry-pick: `6fd4e6f4` local proxy request overrides touches `forwarder.rs`, `ProviderForm.tsx`, `CodexFormFields.tsx`, `types.ts`, locales; `edeee25f` database recovery screen needs early DB-version checks integrated with CCSwitchMulti startup/updater semantics; `a4eb5f37` format selector decoupling must preserve MultiRouter model catalog browser and visible/upstream model split; `273cc48c` native Responses API preset migration must preserve CCSwitchMulti route mapping semantics; `430ddf92`/`dd6a951c` SubRouter/OpenCode Go presets and `142c8c1d` ETok rename should be merged without dropping fork presets/docs.
- Do not take official `f9547da9` version bump literally. The CCSwitchMulti successor should use the fork version scheme, likely `3.16.4-1` if preparing a release from this official base, and update all fork version surfaces consistently (`package.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json`, release notes/export metadata).

## 2026-06-28 CCSwitchMulti v3.16.3-23 Prerelease

- `v3.16.3-23` 已作为 GitHub prerelease 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.3-23`。Release 为非 draft、`prerelease=true`，发布时间为 `2026-06-27T19:50:42Z`，target commit 为 `d8f254fbf9d7b687f385e12bd8df98125306d5f3 build(pnpm): approve release build dependencies`，tag 覆盖 `v3.16.3-22..main` 的 16 个未发布提交。
- 本次发布包含 Codex OAuth 休眠/唤醒与 provider 切换稳定性修复：`get_status()` 保持离线状态语义、`access_token` 只在内存缓存、`RefreshTokenInvalid` 只在真实 token 请求明确 401/403 时清账号；同时移除 `codex_config.rs` 模型 catalog fallback 里的隐藏 live OAuth fetch，避免独立 `CodexOAuthManager` 轮换 refresh token 后主 manager 误删账号。
- Windows 本地 post-commit release pipeline 构建成功，导出目录为 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，完成时间 `2026-06-28 03:49:04 +08:00`。raw exe `CCSwitchMulti_3.16.3-23_x64.exe` 的 FileVersion/ProductVersion 均验证为 `3.16.3-23`，`latest.json` 指向 `v3.16.3-23` 的 Windows setup 资产。
- pnpm 11 在发布前阻止 `esbuild`/`msw` postinstall，修复方式是提交 `pnpm-workspace.yaml` 的 `allowBuilds` / `onlyBuiltDependencies` 白名单，并运行 `pnpm approve-builds --all`、`pnpm install --frozen-lockfile`、`pnpm typecheck` 验证。以后本地 release pipeline 遇到 `ERR_PNPM_IGNORED_BUILDS`，先检查该文件，不要交互式留 placeholder。
- 发布资产当前包含 Windows setup、setup signature、portable zip、raw exe、`latest.json`、`SHA256SUMS.txt`、Linux/macOS build notes；Linux/macOS 二进制资产未在本地生成，需要后续 supplemental workflow 或对应平台构建补齐。发布后复核 `gh release view v3.16.3-23 --repo BigStrongSun/ccswitchmulti --json tagName,targetCommitish,isDraft,isPrerelease,publishedAt,url,assets` 返回 8 个资产。

## 2026-06-28 Codex OAuth Sleep Wake Refresh Invalid Status Fix

- 休眠/唤醒后 Codex OAuth 认证页显示“已登录账号”的原版语义是“本地 `codex_oauth_auth.json` 里仍有账号和 refresh_token 记录”，不是在线验证结果。`get_status()` 不应主动 refresh，也不应因为打开认证页就清理账号；否则状态页会放大 refresh token 使用次数和临时网络误判。
- 最终修复边界：保持原版凭据模型，`refresh_token` 持久化，`access_token` 只在内存缓存。只有真实请求、额度查询、模型查询等需要 Bearer token 的路径调用 `get_valid_token_for_account()`；当 OpenAI token 端点明确返回 401/403 并映射为 `RefreshTokenInvalid` 时，才移除对应账号并让下一次状态查询显示未认证。网络错误、解析错误等临时故障不清空账号。
- 追加排查发现的隐藏边界：`src-tauri/src/codex_config.rs` 生成 Codex provider/model catalog 时不能因为官方 `models_cache.json` 缺失或无 `context_window` 就创建独立 `CodexOAuthManager` 去读取同一份 `codex_oauth_auth.json` 并在线 fetch models。该路径绕过 app 托管的 `CodexOAuthState`，若 refresh token 被官方轮换，主 manager 可能继续持旧 token，后续真实请求会误判 OAuth 失效并清账号。配置/catalog 生成只能读取离线 cache 或测试覆盖值，真实 OAuth refresh 必须通过托管状态发生在用户显式触发的请求/额度/模型查询路径。
- 底层容错也要保留：`CodexOAuthManager::get_valid_token_for_account()` 在 access_token 缓存未命中并拿到账号刷新锁后，应在读取 refresh_token 前重新加载一次 `codex_oauth_auth.json`。这不是为了恢复隐式在线刷新，而是防止未来双进程、旧版本遗留独立 manager 或其他实例已经把 refresh_token 从 A 轮换到 B 后，当前实例继续用内存 A 刷新并触发 `RefreshTokenInvalid` 误删账号。
- 前端 `useManagedAuth` 的 `hasAnyAccount` 不能只等于 `accounts.length > 0`，应受后端 `authenticated` 约束。Codex OAuth 本地账号记录和真实可用认证态必须分开看；以后不要再用“本地有账号”直接驱动绿色认证状态或保存校验。
- 回归测试落点：`codex_oauth_auth.rs` 覆盖 `get_status_does_not_refresh_or_remove_invalid_account`、`token_request_removes_account_when_refresh_token_is_invalid`、`token_request_refreshes_expired_default_account_when_token_is_valid`。验证命令优先跑 `cargo test --manifest-path src-tauri\Cargo.toml token_request_ --lib` 和 `cargo test --manifest-path src-tauri\Cargo.toml get_status_does_not_refresh --lib`。

## 2026-06-27 Logging And Frequent Exit Diagnostics Inventory

- 程序已有三类本地日志：通用运行日志由 `tauri-plugin-log` 写到 `<app_config_dir>/logs/cc-switch.log`（默认 `~/.cc-switch/logs/cc-switch.log`），panic hook 追加写 `<app_config_dir>/crash.log`，Codex MultiRouter 诊断事件写 `<app_config_dir>/logs/codex-router.log`。默认 app config 目录仍是用户家目录下 `.cc-switch`，但启动时会先读取 Store 里的 app_config_dir 覆盖。
- `src-tauri/src/panic_hook.rs` 会在启动最早期安装 panic hook，并强制 `RUST_BACKTRACE=1`；崩溃日志包含时间戳、版本、OS/arch/family、工作目录、线程名/ID、panic message、文件/行/列和完整 backtrace。`src-tauri/Cargo.toml` 设置 `panic = "unwind"`，因此 Rust panic 能被 hook 捕获；但直接进程 abort、系统杀进程、WebView/前端 JS 崩溃不一定进入该 hook。
- 通用日志初始化在 `src-tauri/src/lib.rs` 的 setup 阶段，目标包括 stdout 和日志目录文件 `cc-switch.log`，轮转策略是 `KeepSome(2)`、单文件 1GB。启动后会从 DB 的 `log_config` 读取开关和级别，通过 `log::set_max_level` 应用；前端入口是设置页高级里的 `LogConfigPanel`，只提供启用/禁用和 error/warn/info/debug/trace 级别选择。
- Codex router 日志由 `src-tauri/src/proxy/codex_router_log.rs` 直接追加写入，记录 `route_resolved`、`request_prepared`、`upstream_send`、`upstream_status`、`response_ready` 等清洗后的排障事件；它不会记录 prompt、header 原文或 SSE 内容，并会遮盖 token/API key。MultiRouter 状态页的一键诊断会读取该文件判断近期请求、错误和真实出站协议。
- 现有“异常退出恢复”只针对代理/Live 接管残留：启动时检查 DB live backup 和 live config 占位符，必要时调用 `recover_from_crash()` 恢复配置。这不是通用的频繁退出检测，也不会统计崩溃次数。
- 当前没有现成的“频繁退出/崩溃频率”检测：没有启动 marker、正常退出 marker 清理、退出原因/退出码统一记录、时间窗口计数、watchdog、最近 crash 自动提示，也没有“打开日志目录”的设置页按钮。排查别人频繁退出时，先让对方收集 `~/.cc-switch/crash.log`、`~/.cc-switch/logs/cc-switch.log`，若涉及 Codex MultiRouter 再收集 `~/.cc-switch/logs/codex-router.log`；如果 `crash.log` 没有新条目，就要考虑非 Rust panic 路径（前端/WebView、系统杀进程、安装器重启、进程 abort）。

## 2026-06-28 Abnormal Exit And Crash Cause Logging

- 新增 `src-tauri/src/app_exit_monitor.rs` 作为不依赖数据库的异常退出记录层：启动时写 `<app_config_dir>/logs/app-run-marker.json`，正常退出时删除 marker 并向 `<app_config_dir>/logs/app-exit-events.jsonl` 追加 `clean_exit`，下次启动如果发现 marker 残留则追加 `abnormal_exit_detected` 并在 `cc-switch.log` 打 warn。这样数据库初始化失败、配置迁移失败或 Tauri 事件循环异常退出也能留下证据。
- `panic_hook` 现在除了继续写完整 `<app_config_dir>/crash.log`，还会向 `app-exit-events.jsonl` 写结构化 `panic` 事件，包含 panic message、源码位置和线程摘要；完整 backtrace 仍只在 `crash.log`，避免 JSONL 过大。
- 已挂接的正常/显式退出路径包括窗口关闭退出、用户主动退出、Tauri restart、自定义 `restart_process`、Windows updater install 前退出、旧 config 加载失败用户退出、数据库初始化失败用户退出。系统强杀/abort 仍无法在退出前写 clean event，但会因 marker 残留在下次启动被识别。
- 设置页高级日志配置新增“打开日志目录”入口，调用 `open_log_dir` 打开 `<app_config_dir>/logs`，方便用户收集 `cc-switch.log`、`app-exit-events.jsonl`、`app-run-marker.json` 和 `codex-router.log`。完整 Rust backtrace 的 `crash.log` 仍位于 `<app_config_dir>` 根目录。

## 2026-06-26 CCSwitchMulti v3.16.3-22 Prerelease

- `v3.16.3-22` 已作为 GitHub prerelease 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.3-22`。Release 为非 draft、`prerelease=true`，发布时间为 `2026-06-26T04:16:52Z`，tag 指向 `d4260d1aeb89ade1859f4a341612a8453fc57cbb chore(release): prepare v3.16.3-22 prerelease`。
- 业务修复来自 `9b91ff5d fix(codex): refresh multirouter model sources optimistically`：MultiRouter `/models` 刷新成功后不再等待父级 providers refetch，当前打开的 route picker 会通过 `optimisticModelSourcesById` 立即读到新 catalog，解决“读取成功但 UI 仍显示未发现模型目录 / 卡在旧列表”的边界。
- Windows 本地 post-commit release pipeline 构建成功，导出目录为 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，完成时间 `2026-06-26 11:59:43`；Windows setup 为 `CCSwitchMulti_3.16.3-22_x64-setup.exe`，raw exe 的 FileVersion/ProductVersion 均验证为 `3.16.3-22`，`latest.json` 指向 `v3.16.3-22`。
- 发布创建时 `gh release create` 在 raw exe 上传阶段遇到 EOF 并留下 draft release；恢复方式是停止残留 `gh` 进程，逐个补传缺失 Windows 资产，并删除误名 checksum 资产后重新上传正确的 `SHA256SUMS.txt`。后续 Linux/macOS workflow 又刷新了最终 checksum。
- Supplemental Linux Release workflow `28216822549` 成功，上传 AppImage、deb、rpm；Supplemental macOS Release workflow `28216824340` 成功，上传 unsigned universal `.app.zip`、universal updater tarball 和 `.tar.gz.sig`。最终 release 共有 12 个资产，`SHA256SUMS.txt` 覆盖除自身外的 11 个资产。
- 发布前验证：`pnpm test:unit -- src/components/codex/CodexRouterWorkspacePage.test.ts`（21 个测试通过）、`pnpm typecheck`、`cargo check --manifest-path src-tauri/Cargo.toml`（仅既有 `commands/misc.rs` unused warnings）、`git diff --check`（仅 `Cargo.lock` LF/CRLF 提示）。发布后复核：`gh release view v3.16.3-22 --repo BigStrongSun/ccswitchmulti --json tagName,isDraft,isPrerelease,url,assets,publishedAt,targetCommitish`、下载 `SHA256SUMS.txt` 检查 11 条 checksum、`gh run view 28216822549` 和 `gh run view 28216824340` 均为 `status=completed, conclusion=success`。

## 2026-06-26 MultiRouter Model Refresh UI Stale Catalog Fix

- 新版仍出现“加载模型列表卡住 / UI 没刷新”时，要区分两类问题：`v3.16.3-21` 已解决 `/models` 读取或保存事务不 settle 导致永久 loading；本次发现的剩余边界是刷新成功后 `nextProvider` 写入 DB/React Query，但当前 `CodexRouterWorkspacePage` 的 `modelSources` 仍可能来自父级旧 `providers` props，导致已打开的 `RouteCandidatePicker` 继续显示旧 catalog 或“未发现模型目录”。
- 根因位置是 `src/components/codex/CodexRouterWorkspacePage.tsx`：旧 `effectiveProviders` 只叠加 `optimisticRoutingPlan`，没有叠加普通模型源的刷新结果；同时刷新成功分支的 `queryClient.setQueryData(["providers","codex"])` 在 cache 尚无 `providers` 字段时会返回旧引用，不能保证触发 UI 更新。
- 修复方式是新增 `optimisticModelSourcesById`，在 `fetchModelsForConfig -> providersApi.update(nextProvider)` 成功后立即把普通 provider 的新 catalog 叠加进 `effectiveProviders`，让候选 router 和空 match route 立刻读取新模型；当父级 props 的 catalog 追上或 provider 连接配置/baseUrl/API key 变化时自动释放 overlay，避免旧 catalog 长期压住新配置。
- 回归测试新增 `refreshes visible route picker candidates after provider catalog save without parent refetch`：provider 初始 catalog 为空，打开候选选择器时显示“未发现模型目录”，`/models` 返回 `fresh-route-model` 且保存成功后，在不模拟父级 refetch 的情况下候选卡片必须立刻显示 `fresh-route-model` 并移除空目录提示。
- 本轮验证：`pnpm test:unit -- src/components/codex/CodexRouterWorkspacePage.test.ts`（21 个测试通过）、`pnpm typecheck`、`pnpm build:renderer`、`git diff --check`。renderer build 仍只有既有 baseline/browserlist/大 chunk 警告。

## 2026-06-25 CCSwitchMulti v3.16.3-21 Prerelease

- `v3.16.3-21` 已作为 GitHub prerelease 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.3-21`。tag 指向 `554bed1c chore(release): prepare v3.16.3-21 hotfix`，业务修复来自 `966a8e38 fix(codex): settle model refresh save-back hangs`。
- 本次热修的真实边界：`v3.16.3-20` 只修了并发刷新和 `/models` 阶段超时，仍可能在读取成功后的 `providersApi.update` 写回 provider / plan catalog 阶段永久 loading；`v3.16.3-21` 才把读取和写回合成同一个 30 秒超时事务。
- Windows 本地 release hook 构建成功，导出目录为 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，raw exe 文件版本为 `3.16.3-21`。release 创建时 `gh release create` 曾在 raw exe 上传阶段卡住，留下 draft；处理方式是停止残留 `gh`，补传 raw exe，再 `gh release edit --draft=false --prerelease=true` 发布。
- Supplemental Linux Release workflow `28177240622` 成功并上传 AppImage、deb、rpm；Supplemental macOS Release workflow `28177240635` 成功并上传 unsigned universal `.app.zip`、updater tarball 和 tarball 签名。最终 release 共有 12 个资产，`SHA256SUMS.txt` 覆盖除自身外的 11 个资产。
- 发布前验证：`pnpm test:unit -- src/components/codex/CodexRouterWorkspacePage.test.ts`、`pnpm typecheck`、`cargo check --manifest-path src-tauri/Cargo.toml`（仅既有 `commands/misc.rs` unused warnings）、`git diff --check`、`pnpm build:renderer`。

## 2026-06-25 MultiRouter Model Refresh v3.16.3-21 Hotfix Boundary

- 用户/外部反馈截图仍停在“候选 provider 模型列表刷新 / 正在读取模型列表...”时，必须区分三个版本边界：本机安装目录 `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe` 仍是 `3.16.3-18`，公开 `v3.16.3-19` 完全不含刷新状态机修复，公开 `v3.16.3-20` 含 `ddfeed42`/`33a0bc58` 但不含 `966a8e38 fix(codex): settle model refresh save-back hangs`。
- `v3.16.3-20` 的 `withModelRefreshTimeout` 只包住 `fetchModelsForConfig(...)`，读取成功后的 `providersApi.update(nextProvider)` 与受影响 plan 写回仍可能永久挂起；当前 HEAD `966a8e38` 才把读取、provider catalog 写回、MultiRouter plan catalog 写回合成同一个 30 秒超时事务，并显示“已读取 N 个模型，正在写回本地配置...”阶段文案。
- 本轮现场验证：`pnpm test:unit -- src/components/codex/CodexRouterWorkspacePage.test.ts` 通过 20 个测试；截图类问题应通过补发 `v3.16.3-21` 处理，release notes 不能再建议 save-back 卡住用户只升级到 `v3.16.3-20`。

## 2026-06-25 MultiRouter Model Refresh Save-Back Timeout Fix

- MultiRouter 路由页“候选 provider 模型列表刷新”卡在“正在读取模型列表...”不只可能发生在 `/models` IPC/网络阶段；`src/components/codex/CodexRouterWorkspacePage.tsx` 在读取成功后还会 `providersApi.update` 写回普通 provider 的 `modelCatalog`，并重建/写回受影响 MultiRouter plan 的 `modelCatalog`。旧 `withModelRefreshTimeout` 只包住 `fetchModelsForConfig`，如果后续 provider/plan 保存、Codex live catalog/cache 同步或本地 DB/文件写入挂起，UI 仍会永久停留在 loading。
- 当前修复把“读取 `/models` -> 写回 provider catalog -> 写回受影响路由方案”视作一个刷新事务，30 秒超时覆盖整个事务；读取完成进入保存阶段时，卡片文案改为“已读取 N 个模型，正在写回本地配置...”，避免把保存阶段误判成远端 `/models` 还在读。
- 超时 attempt 会被记录到 `modelRefreshTimedOutAttemptKeysRef`，后台迟到的 Promise 不允许再把 error/loading 覆盖成 success；同时 catch 只在该 provider 仍然是当前 attempt 时写错误态，避免旧 attempt 超时覆盖新 attempt。
- 回归测试 `src/components/codex/CodexRouterWorkspacePage.test.ts` 覆盖两类永久 loading 边界：`fetchModelsForConfig` 永不返回，以及 `providersApi.update` 写回刷新结果永不返回。后者会先显示写回阶段文案，30 秒后落到错误态，迟到 resolve 不能再变成成功态。

## 2026-06-25 Codex Catalog Visible Alias And Upstream Model Split

- 第三方 Codex provider 的 `modelCatalog.models[].model` 是 Codex/子 Agent 可见候选名，不再强制等于真实上游模型名；新增 `upstreamModel`/`upstream_model` 表示请求发往上游时使用的模型。为空或等于 `model` 时按旧配置兼容处理。
- 普通表单和 MultiRouter 自动 `/models` 刷新合并时必须按 `upstreamModel || upstream_model || model` 优先匹配远端返回的 id，避免用户把 `gpt-5.5` 改成 `gpt-5.5-thirdparty` 后，下一次刷新又新增一个重复的 `gpt-5.5` 或把别名覆盖掉。新增远端模型默认写成 `model=id, upstreamModel=id`，保存时若二者相同会省略 upstream 字段。
- 运行时出站映射顺序固定为：route 级 `codexResolvedUpstreamModelOverride` / `modelMap` 优先，其次 catalog 条目的 `upstreamModel`，最后回退到 provider/config 里的单模型字段。这个映射必须同时覆盖 Responses 原生直连和 Responses->Chat 转换路径。
- Codex catalog 文件可以携带 `upstreamModel` 作为 cc-switch 私有元数据，但 OpenAI-compatible `/v1/models` 的 `data[]` 只能暴露可见模型名和上下文窗口，不应把真实 upstream alias 暴露出去。

## 2026-06-25 MultiRouter Model Refresh Release Boundary And Timeout Guard

- 用户/他人看到 MultiRouter 路由页“候选 provider 模型列表刷新”一直卡在“正在读取模型列表...”时，先确认运行版本；`v3.16.3-19` tag 指向 `6a1cf4e1`，不包含本地 `ddfeed42 fix(codex): settle multirouter model refresh states`，而本机安装目录 `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe` 仍是 `3.16.3-18`，所以截图类问题很可能是发布包未带修复而不是 HEAD 修复失效。
- 2026-06-25 再次确认：GitHub prerelease `v3.16.3-19` 的 target commit 仍是 `6a1cf4e1`，`ddfeed42` 和 `33a0bc58` 都不在该 tag 内；别人发来的两个 provider 同时显示“正在读取模型列表...”的截图，应优先按“公开包未发布刷新状态机修复”处理。下一次发版必须包含 `ddfeed42`/`33a0bc58`，否则该问题会继续在已安装包里出现。
- `src/components/codex/CodexRouterWorkspacePage.tsx` 的候选 provider 自动刷新现在有双层保护：per-provider active attemptKey 负责防止 rerender cleanup 吞掉 pending 请求终态；前端 `withModelRefreshTimeout` 再给 IPC/后端异常挂起加 30s 兜底，必须让 UI 从 loading 落到错误态。
- attemptKey 不能只记录 `Boolean(apiKey)`；API Key 从一个非空值换成另一个非空值时必须重新发起 `/models` 读取，并让旧请求结果无法写回。当前实现对 API Key 做短哈希后参与内存态 attemptKey，不持久化也不展示完整密钥。
- 回归测试在 `src/components/codex/CodexRouterWorkspacePage.test.ts` 增加两类边界：API Key 变化时 stale request 不写回，以及 `fetchModelsForConfig` 永不返回时 30s 后显示错误而不是永久 loading。

## 2026-06-25 MultiRouter Candidate Model Refresh Loading Fix

- MultiRouter 路由页“候选 provider 模型列表刷新”一直停在“正在读取模型列表...”的根因在前端并发刷新状态机，不是后端 `/models` 请求缺少超时。后端 `src-tauri/src/services/model_fetch.rs` 每个请求已有 15s timeout；问题是 `src/components/codex/CodexRouterWorkspacePage.tsx` 自动刷新多个 provider 时，第一个 provider 成功写回 `providersApi.update` / `setOptimisticRoutingPlan` 会触发 effect cleanup，旧实现用局部 `cancelled` 阻断后续 pending provider 的 `.then/.catch`，而新一轮 effect 又被 `modelRefreshAttemptedKeysRef` 去重跳过，于是 UI 永久留在 loading。
- 修复方式是按 provider 维护当前最新 `attemptKey`，用 `modelRefreshActiveAttemptKeysRef` 判断请求是否仍是该 provider 的最新 attempt；正常 rerender 不再吞掉同批并发请求终态，真实配置变更产生的新 attempt 仍能阻止旧请求覆盖状态或写回 DB。
- 回归测试在 `src/components/codex/CodexRouterWorkspacePage.test.ts` 用可手动 resolve/reject 的 Promise 复现两个 provider 并发：Provider A 先成功并触发 rerender 后，Provider B 后续成功必须显示 `已读取并更新 1 个模型。` 且写回；Provider B 后续失败必须显示错误而不是卡 loading。
- 本轮验证：`pnpm test:unit -- src/components/codex/CodexRouterWorkspacePage.test.ts`、`pnpm typecheck`、`pnpm build:renderer`、`git diff --check`。renderer build 仍只有既有 baseline/browserlist/chunk 警告。

## 2026-06-25 CCSwitchMulti v3.16.3-19 Prerelease

- `v3.16.3-19` 已作为 GitHub prerelease 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.3-19`。tag 指向版本 bump 提交 `6a1cf4e1`，版本面同步点仍是四处：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`。业务修复提交是 `2e9723c1`（MultiRouter 子 Agent 流量监控 + 浅色主题修复），前面还包含 vLLM/Qwen 上下文窗口修复提交 `7481bbb5`、`6d5d8c02`。
- 本次 release notes 必须继续用中文。内容覆盖：MultiRouter “今日子 Agent 会话流量”、子 Agent/模型聚合、会话用量同步入口、浅色模式可读性修复、vLLM `max_model_len/maxModelLen` 上下文窗口读取、SQLite session_id 分块查询，以及 macOS universal history-repair sidecar 构建修复。
- 本地 Windows 构建路径：`powershell -NoProfile -ExecutionPolicy Bypass -File scripts/local-release-pipeline.ps1 -ReleaseRoot C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.3-19 -Reason manual-prerelease-v3.16.3-19`。产出被整理到 `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.3-19-assets`，包括 setup、setup.sig、portable zip、raw exe、`latest.json`。
- Linux 资产在 WSL `openclaw` 内完成，仍然使用临时 `{"bundle":{"createUpdaterArtifacts":false}}` 配置构建：先 `cargo build --manifest-path src-tauri/Cargo.toml --bin codex-history-repairer --features history-repairer --release`，再 `pnpm tauri build --bundles appimage,deb,rpm --config <tmpfile>`。实际上传资产是 `CCSwitchMulti_3.16.3-19_amd64.AppImage`、`CCSwitchMulti_3.16.3-19_amd64.deb`、`CCSwitchMulti-3.16.3-19-1.x86_64.rpm`。
- macOS 本机仍不能构建；这次通过 `Supplemental macOS Release` workflow_dispatch 构建并上传，run id `28150527263` 成功，耗时约 29m30s。该 workflow 上传了 `CCSwitchMulti_3.16.3-19_universal.tar.gz`、`.tar.gz.sig`、`CCSwitchMulti_3.16.3-19_universal.app.zip`，并刷新 release `SHA256SUMS.txt`。
- 最终 release 资产数为 12：Windows 4 个、Linux 3 个、macOS 3 个、`latest.json`、`SHA256SUMS.txt`。tag/main push 触发的 `.github/workflows/release.yml` push run 仍出现无 job 的失败记录，不作为本次发布路径；本次实际发布路径是手动本地 Windows + WSL Linux + supplemental macOS。
- 本轮验证：`pnpm typecheck`、`pnpm build:renderer`、`cargo check --manifest-path src-tauri/Cargo.toml`、`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo test --manifest-path src-tauri/Cargo.toml codex_subagent_usage_stats --lib`、`git diff --check`。已知非阻塞警告仍是 Rust unused helper、Vite browserslist/baseline 和大 chunk 警告，以及 Tauri bundler `__TAURI_BUNDLE_TYPE` warning。

## 2026-06-24 MultiRouter Subagent Usage And Light Theme Readability

- MultiRouter 状态页的子 Agent 流量监控不能从真实代理转发日志里直接推断身份；真实代理日志只回答 route/provider/model 的出站归属。子 Agent 监控的来源应固定为 Codex 本地历史 SQLite/JSONL：先用 `thread_source="subagent"` 或 JSONL `session_meta.payload.source.subagent.thread_spawn` 确认子 Agent，再只聚合 `proxy_request_logs` 中 `app_type='codex'`、`data_source='codex_session'`、`session_id IN (subagent session ids)` 的同步用量行。
- 子 Agent 监控的 UI 口径是“本地 Codex 会话 token_count 同步后的用量”，不是代理层实际请求转发次数；因此页面需要保留“今日子 Provider / Model 流量”和“今日子 Agent 会话流量”两个分区，前者看真实出站，后者看子 Agent/模型消耗。
- MultiRouter 页面和第三方 Agent API 页面浅色模式修复应优先使用 `bg-card`、`bg-muted`、`bg-background`、`text-foreground`、`text-muted-foreground`、`border-border` 等语义 token，再把原来的深色透明样式放进 `dark:` 变体。不要在浅色主类里继续使用 `bg-slate-950/*`、`text-slate-100`、`text-white` 或深色半透明卡片。
- 子 Agent 会话统计查询 `session_id IN (...)` 时必须分块，当前保守批量是 500；`get_codex_subagent_usage_stats` 默认会为了状态页读取最多 1600 条历史、最多 5000 条只读候选，因此不要把所有 session_id 一次塞进 SQLite 变量绑定。
- 本轮验证基线：`pnpm typecheck`、`pnpm build:renderer`、`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo test --manifest-path src-tauri/Cargo.toml test_codex_subagent_usage_stats_only_counts_subagent_session_rows --lib`、`cargo check --manifest-path src-tauri/Cargo.toml`。Rust 只剩既有 `commands/misc.rs` unused 警告；renderer build 只剩既有 browserslist/baseline 和大 chunk 警告。

## 2026-06-24 CCSwitchMulti v3.16.3-18 GitHub Release

- 远端 `BigStrongSun/ccswitchmulti` 已经存在 `v3.16.3-17` prerelease（含本地 Windows/Linux 资产），因此这次不能复用旧 tag；新的正式 release 需要前进到 `v3.16.3-18`。版本面同步点仍是四处：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`。
- `v3.16.3-18` GitHub Release 已发布为 Latest：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.3-18`。release tag 指向提交 `6ff4252f`（版本 bump + unsigned macOS workflow 基线），后续 workflow 修复继续落在 `main` 上的 `93ec101b`，然后用 `workflow_dispatch` 构建同一个 tag 的补充资产。
- 本地 Windows 构建由 post-commit release hook 自动触发成功，随后用 `scripts/export-latest-ccswitchmulti.ps1 -SkipBuild -ReleaseRoot C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.3-18` 固化干净版本目录。发布 staging 目录是 `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.3-18-assets`，只保留本次 release 实际上传的 Windows/Linux 资产与 `latest.json`。
- 本地 Linux 构建是在 WSL `openclaw` 中完成的，命令路径要先补 `PATH=\"$HOME/.cargo/bin:$PATH\"`，再构建 `codex-history-repairer` sidecar，然后用临时 `{\"bundle\":{\"createUpdaterArtifacts\":false}}` 配置执行 `pnpm tauri build --bundles appimage,deb,rpm --config <tmpfile>`。这次实际产物是 `CCSwitchMulti_3.16.3-18_amd64.AppImage`、`CCSwitchMulti_3.16.3-18_amd64.deb`、`CCSwitchMulti-3.16.3-18-1.x86_64.rpm`。
- macOS 本地构建在这台 Windows 主机上仍然不可行，硬边界是 Tauri 需要目标平台原生运行时和 macOS SDK/WebKit，而不是“少装一个 Rust target”。能完成的是 GitHub macOS runner 上的 unsigned supplemental build。
- 第一次 supplemental macOS workflow（run `28094163276`）失败的真实根因不是签名，而是 universal 打包阶段缺少 `src-tauri/target/universal-apple-darwin/release/codex-history-repairer`。修复方式不是重试，而是在 `.github/workflows/release.yml` 和 `.github/workflows/supplemental-macos-release.yml` 中都显式为 `codex-history-repairer` 构建 `aarch64-apple-darwin` 与 `x86_64-apple-darwin`，再用 `lipo` 合成 universal sidecar。
- 第二次 supplemental macOS workflow（run `28095435446`）成功后，release 额外补齐了 unsigned macOS 资产：`CCSwitchMulti_3.16.3-18_universal.tar.gz`、`CCSwitchMulti_3.16.3-18_universal.tar.gz.sig`、`CCSwitchMulti_3.16.3-18_universal.app.zip`，并自动刷新了 `SHA256SUMS.txt`。最终 release 共有 12 个资产：Windows 4 个、Linux 3 个、macOS 3 个、`latest.json`、`SHA256SUMS.txt`。
- 这条发布线还有两个环境约束要记住：一是 `.github` 被仓库 `.gitignore` 忽略，新增或修改 workflow 时必须 `git add -f .github/workflows/...`；二是本地 `post-commit` hook 会自动跑 `scripts/local-release-pipeline.ps1` 并占用 `scripts/logs/local-release.lock`，发布期间不要并发再起第二个本地构建。

## 2026-06-24 MultiRouter Protocol Probe And Codex Responses Decision Unification

- 当前 Codex MultiRouter 的 `/responses` -> 上游协议选择，本质上一直是“配置判定”，不是在线能力探测。单一真理来源现在收敛到 `src-tauri/src/proxy/providers/codex.rs::explain_codex_responses_upstream_protocol`：优先级为 managed `codex_oauth` 直连官方 `responses` > `meta.apiFormat` > `settings_config.api_format/apiFormat` > 已知 Chat Completions-only `base_url` > `config.toml wire_api` > 默认 `responses`。
- 这次修复顺手把一个关键边界钉死：只要 provider 被识别为 managed Codex OAuth，哪怕残留了 `apiFormat=openai_chat` 之类污染字段，也必须保持原生 `chatgpt.com/backend-api/codex/responses` 透传，不能再被误转成 `/v1/chat/completions`。
- `src-tauri/src/commands/proxy.rs` 的 MultiRouter 诊断现在会为每条 route 返回 `configuredProtocol/configuredProtocolSource/configuredProtocolDetail`，而且 route 摘要不再自己猜 target provider 配置，而是通过与运行态一致的 `build_codex_route_probe_provider` 物化 effective provider 后再判定。
- `codex-router.log` 的 `request_prepared` 事件原本就包含 `effective_endpoint`、`upstream_url`、`responses_to_chat`、`responses_to_messages`。现在诊断层会把这些字段解析成 `actualProtocol`，前端状态页“协议探测”视图可按每个 `Provider + Model` 展示“配置判定”与“最近实测”，直接看出最后一次真实出站走的是 `responses`、`chat` 还是 `messages`。
- 状态页里的“协议探测”按钮不会主动消耗真实上游额度；它只读取当前 route 配置和最近 router 日志。因此它解决的是“当前代码会怎么判、最近一次实际怎么走”的可见性问题，不是远端能力协商。如果后续真要做在线 capabilities probe，需要单独设计安全的探测请求与缓存。

## 2026-06-24 Codex Official Context Window Live Fallback

- 在 `src-tauri/src/codex_config.rs` 中，官方 GPT/Codex 模型的上下文窗口读取链现在是：provider DB 显式 `contextWindow` > `~/.codex/models_cache.json` > 本机已登录 Codex OAuth 账号实时拉取 `https://chatgpt.com/backend-api/codex/models` > `config.toml` 的 `model_context_window` > 最终默认值 `128000`。
- 这条 live fallback 专门覆盖首次配置、用户清理 `models_cache.json`、缓存损坏、以及缓存里只有 slug 但缺少 `context_window` 的场景，避免 Codex Desktop 又回退成 128k/约 122k 的显示。
- 同步桥接异步 OAuth 拉模时，不要在已有 Tokio runtime 里直接嵌套 `block_on` 或用当前线程硬顶网络 future。当前实现改为把 live 官方读取放到独立线程，再在该线程里使用 `tauri::async_runtime::block_on`，这样不会污染调用侧 runtime，也更适合 Tauri 同步配置生成路径。
- 回归测试必须至少覆盖三类边界：`models_cache.json` 缺失、JSON 损坏、缓存存在但缺失上下文字段；三种情况下都应能从 live OAuth 元数据恢复 `context_window`。

## 2026-06-24 Release Workflow Fork Secret Degradation

- `fork` 仓库的 release matrix 不能假设一定有 Apple 签名/公证 secrets。若 `APPLE_CERTIFICATE` 一类 secret 为空，旧 workflow 会在 `Import Apple signing certificate` 直接失败，并因为 matrix 默认 `fail-fast` 取消掉本来还能完成的 Windows/Linux 打包。
- 修复策略：`release.yml` 里将矩阵改为 `fail-fast: false`；macOS 证书导入、DMG 公证、签名验证只在 Apple secrets 和 `APPLE_SIGNING_IDENTITY` 都存在时执行。缺少签名材料时，macOS 仍然产出 updater `.tar.gz` 和 `.zip`，但跳过 `.dmg`、公证与签名校验，不再拖死整条 release。

## 2026-06-24 Codex Official GPT Context Window Projection Fix

- 现场现象：Codex Desktop 里官方链路/Multirouter 的 `gpt-5.5` 显示约 122k 上下文，而 CCSwitchMulti live `config.toml` 和 `cc-switch-model-catalog.json` 中 `gpt-5.5` 已是 272000。122k 与 128000 的 `effective_context_window_percent=95` 接近，说明 Desktop 某条读取路径忽略了 272000 后回退到了默认 128k。
- 根因边界：`src-tauri/src/proxy/handlers.rs` 的 Codex client `GET /v1/models` 会把 cc-switch catalog 扩展成 OpenAI-compatible `data[]`。该 `data[]` 以前只写 `context_window` / `max_context_window`，没有 `contextWindow` / `maxContextWindow`。Codex Desktop 某些 renderer/app-server 路径读取 `data[]` 时会看 camelCase 字段，读不到就按默认 128k 再乘 95% 展示。
- 修复：`openai_model_entry_with_source` 在 `data[]` model entry 中同时投 snake_case 与 camelCase：`context_window`、`max_context_window`、`contextWindow`、`maxContextWindow`。这不改变 raw `models[]` catalog 和已有外部 OpenAI API 兼容字段，只补齐 Desktop 读取别名。
- 回归测试：`proxy::handlers::tests::codex_catalog_models_response_keeps_catalog_and_openai_data` 必须断言四个上下文字段都存在并等于源 catalog 值。
- 后续根治：`src-tauri/src/codex_config.rs` 生成 catalog spec 时，官方 GPT/Codex 模型若 DB `modelCatalog` 未显式写 contextWindow，应优先读取 Codex 官方 `models_cache.json` 的动态上下文窗口，再回退到 `model_context_window` / 128000。不要继续把 `272000` 等 OpenAI 数值当成唯一真实来源。

## 2026-06-24 Qwen Local Context Window Fetch Fix

- 用户现场把问题边界收紧到“获取模型列表阶段没拿到 `qwen3.6=262144`，导致 Codex catalog/压缩阈值先错了”，而不是单纯的 `/responses -> chat` 输出预算裁剪。上游报错里出现的 `262144` 只是运行时错误文本，本地之前不会把它自动回写到 provider catalog。
- 直接探测用户这条 vLLM 端点 `https://www.matrixminecraft.cn:24443/vllm/v1/models` 后确认：远端其实已经返回了 `max_model_len: 262144`，并不是“vLLM 没给上下文窗口”。根因是 `src-tauri/src/services/model_fetch.rs::extract_context_window` 只识别 `context_window/max_context_window/contextWindow/maxContextWindow`，没识别 vLLM 的 `max_model_len/maxModelLen`。
- 因此正确修复不是给 `qwen3.6` 做应用级静态兜底，而是在配置阶段的真实 `/models` 读取里补上 vLLM 字段解析。这样点“获取模型列表”时就能直接把 `262144` 写进 provider catalog，MultiRouter 和 Codex picker 后续都读取真实值。
- 回归测试改为覆盖 `max_model_len` 和 `maxModelLen` 两种 vLLM 风格字段；`pnpm test:unit -- tests/utils/codexModelContext.test.ts tests/utils/codexSpawnAgentCandidates.test.ts`、`pnpm typecheck`、`cargo test --manifest-path src-tauri/Cargo.toml switching_codex_router_provider_auto_enables_dedicated_local_takeover --lib` 全部通过。

## 2026-06-24 Codex Provider Model Context Window Fallback

- 根因：DeepSeek 等 OpenAI-compatible provider 的 `/models` 端点仅返回模型 id（如 `deepseek-chat`、`deepseek-reasoner`、`deepseek-v4-flash`），不承诺返回 `context_window` 字段。而 Codex provider 表单的"获取模型列表"按钮和 MultiRouter 工作台的自动候选刷新都只在 `fetched.contextWindow` 为 truthy 时才写入上下文窗口，远端没给就留空。
- 修复策略：引入共用工具 `src/utils/codexModelContext.ts`，为 `mergeFetchedModelsIntoCatalogRows`（普通表单）和 `providerWithFetchedModelCatalog`（MultiRouter 候选刷新）提供统一的上下文推断优先级：远端显式值 > 用户已有目录值 > 本地 provider/model 预设元数据。预设匹配会对比 providerId/name/baseUrl/websiteUrl 信号以避免同名模型跨供应商误套。
- DeepSeek 兼容别名（`deepseek-chat`、`deepseek-reasoner`）也在工具中写入了显式 1M 上下文映射，不会因为上游返回旧式 id 而丢上下文。
- 测试 `tests/utils/codexModelContext.test.ts` 覆盖：远端显式值优先、已有目录保留、DeepSeek 预设兜底、DeepSeek 别名兜底、未知模型不捏造上下文。
- 相关提交：该修复同时变更 `CodexFormFields.tsx` 和 `CodexRouterWorkspacePage.tsx`，让两处上下文合并逻辑共用同一推断函数。

## 2026-06-24 Empty Codex Official Seed OAuth Routing Fix

- v3.16.3-15 的 official/OAuth materialize 修复仍有一个漏网条件：全新安装或恢复后的 `codex-official` 可能只是 `category="official"` 的空 seed provider，`settings_config.auth` 为空且没有 `base_url`，真实 OAuth 账号在 CCSwitchMulti 的 `CodexOAuthManager` 存储里。旧判断只认 `meta.providerType="codex_oauth"`、provider 内 `auth.auth_mode="chatgpt"` / tokens，或 router provider 自身的 managed auth，因此空 seed 被误当普通 Codex provider，GPT 原生 route 命中后仍会在 `CodexAdapter::extract_base_url` 报 `Codex Provider 缺少 base_url 配置`。
- 修复应把 `category == "official"` 且 id/name/route target 明确标记 `codex-official` / `OpenAI Official` 的空 seed 识别为 managed Codex OAuth，但继续让带真实非本地 `base_url` 的 provider 走普通第三方路径，避免误伤自定义 OpenAI-compatible provider。
- 回归测试要覆盖两条路径：MultiRouter `targetProviderId="codex-official"` 命中空 official seed 后 materialize 成 `meta.provider_type="codex_oauth"`；以及直接对空 official seed 调 `CodexAdapter` 时返回 `https://chatgpt.com/backend-api/codex` 和 `AuthStrategy::CodexOAuth`。

## 2026-06-24 Qwen MultiRouter Live Route Check

- 用户现场怀疑 MultiRouter 到 `qwen3.6` 的请求没有真正发出去。只读复查确认当前 live `~/.codex/config.toml` 已指向 `model_provider = "codex_model_router_v2"` 和 `base_url = "http://127.0.0.1:15721/v1"`，`cc-switch.exe` 进程 `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe` 同时监听 `15721` 与 `15722`，`http://127.0.0.1:15721/health` 返回 200。
- 当前 DB 里 `codex-openai-router` 是 Codex current provider，`settings_config.codexRouting` 为对象 schema；`qwen-local` route 启用，匹配 `qwen3.6` / `qwen` 前缀，上游为 `https://www.matrixminecraft.cn:24443/vllm/v1`，`wire_api=openai_chat`，并保留 `codexChatReasoning` 的 `enable_thinking` 与 `minOutputTokens=2048`。
- 真实 `spawn_agent model=qwen3.6` 极小请求返回 `OK`。同一时间 `~/.cc-switch/logs/codex-router.log` 出现完整链路：`route_resolved route_id=qwen-local route_missed=false`、`request_prepared upstream_url=https://www.matrixminecraft.cn:24443/vllm/v1/chat/completions responses_to_chat=true`、`auth_prepared auth_strategy=Bearer`、`upstream_send`、`upstream_status status=200`、`response_ready status=200`。这证明当前 MultiRouter 路由层和 15721 转发链路是通的，请求确实进了 qwen 上游。
- 本轮直接探测 `https://www.matrixminecraft.cn:24443/vllm/v1/models` 曾先返回 502，随后返回 200 且列出 `qwen3.6`；因此“卡住/没反应”更像上游 vLLM/relay 短暂抖动、模型冷启动或当时请求未实际选择/发出 qwen，而不是当前 MultiRouter 配置缺 route。后续复现时优先抓失败时刻的 `codex-router.log`：若没有 `model=qwen3.6` 新行，问题在 Codex/子 Agent 发起前；若有 `upstream_send` 但无 200，则看上游状态、首包超时或 502/521。

## 2026-06-24 CCSwitchMulti 3.16.3-15 GitHub Release

- Published `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.3-15` from local `main` after pushing commit `0739638b` and annotated tag `v3.16.3-15` to the `fork` remote (`https://github.com/BigStrongSun/ccswitchmulti.git`).
- This release is the hotfix successor to `3.16.3-14` for Codex MultiRouter regressions. It includes legacy array-shaped `settings_config.codexRouting` compatibility, Rust route resolver support before UI resave, official/OAuth target provider local-proxy pollution handling, and follow-up diagnostics hardening.
- Verification before release: `pnpm typecheck`, `pnpm vitest run src/components/codex/CodexRouterWorkspacePage.test.ts tests/components/useCodexConfigState.test.ts`, `cargo fmt --manifest-path src-tauri\Cargo.toml --check`, and `cargo test --manifest-path src-tauri\Cargo.toml codex_route --lib`.
- Windows export root: `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.3-15`. The first full export timed out at the shell after 15 minutes while `cargo/rustc` was still running; after the build and NSIS processes finished, rerunning `scripts\export-latest-ccswitchmulti.ps1 -ReleaseRoot ... -SkipBuild` completed export, signing, `latest.json`, and checksum generation.
- Initial Windows-hosted upload included `CCSwitchMulti_3.16.3-15_x64-setup.exe`, `.sig`, `CCSwitchMulti_3.16.3-15_x64-portable.zip`, `CCSwitchMulti_3.16.3-15_x64.exe`, `latest.json`, `README.md`, `linux-build-note.md`, `macos-build-note.md`, and `SHA256SUMS.txt`.
- Follow-up Linux supplement: the local WSL build produced AppImage/deb/rpm after first building the `codex-history-repairer` sidecar, but local uploads to `uploads.github.com` repeatedly stalled or disconnected. Commit `ffc2fa0e` added `.github/workflows/supplemental-linux-release.yml`, then GitHub Actions run `28076382107` built and uploaded Linux x86_64 assets for `v3.16.3-15`.
- Final release assets include Linux x86_64 `CCSwitchMulti_3.16.3-15_amd64.AppImage`, `CCSwitchMulti_3.16.3-15_amd64.deb`, and `CCSwitchMulti-3.16.3-15-1.x86_64.rpm`; `linux-build-note.md` was removed after the real Linux packages were uploaded. macOS aarch64 `dmg` and `.app.zip` assets are also present on the release.
- Important release hygiene: the export script's default `SHA256SUMS.txt` is a full export-tree checksum and may include internal tool files or nested platform notes that are not uploaded as release assets. For `v3.16.3-15`, `SHA256SUMS.txt` was regenerated from GitHub release asset digests so every entry corresponds to a downloadable asset.

## 2026-06-23 CCSwitchMulti 3.16.3-14 MultiRouter Route Regression

- `3.16.3-14` 的用户现场证明存在真实回归：MultiRouter provider 仍存在，但 `settings_config.codexRouting` 可能被保存成扁平数组，缺少新版对象外壳 `{ enabled, routes, defaultRouteId }`。新版前后端若只按对象 schema 读取，会表现为 `routing_configured=false` 或 `route_missed=true`，随后请求回落到 MultiRouter provider 自身；MultiRouter 自身不是普通上游，没有真实外部 `base_url`，会报 `Codex Provider 缺少 base_url 配置` 或递归保护 400/502。
- 根因不是 DeepSeek key、网络、用户教程步骤或必须删库重配。现场“直接写 SQLite 把 codexRouting 修成对象”只能作为临时恢复，产品修复必须兼容已损坏/旧式数组 schema，并在 UI 保存时自动迁移回对象 schema。
- 修复点：`CodexRouterWorkspacePage.readCodexRouting` 和 `useCodexConfigState.extractCodexRoutingConfig` 都必须先判断 `Array.isArray(codexRouting)`，将 legacy route 数组迁移成 `{ enabled: true, routes: [...] }`，避免 `typeof [] === "object"` 路径把 routes 清空。Rust `proxy/providers/codex.rs` 的 route resolver 也必须直接消费数组型 `codexRouting`，这样用户未重新保存 DB 前请求链路也能恢复。
- 第二层现场污染：OpenAI Official target provider 可能被写入本地接管代理 `127.0.0.1:15721`，导致 GPT route 命中后又递归回本机代理。route materialize 时，official/OAuth 目标 provider 的本地 proxy `base_url` 不能被当作真实上游；应按托管 Codex OAuth 处理并让 `CodexAdapter` 使用 `https://chatgpt.com/backend-api/codex`。
- 回归测试应覆盖：前端读取 legacy array 不丢 route；表单初始化 legacy array 不清空 route；后端 resolver 能用 legacy array 匹配 GPT/DeepSeek；official target provider 带本地 proxy `base_url` 时仍 materialize 为 `codex_oauth`。

## 2026-06-23 Codex History Repair State DB Auto Detection

- Codex history repair must not hard-code `~/.codex/sqlite/state_5.sqlite` as the default active DB. macOS user reports and upstream Codex issue evidence point to the current default state DB at `~/.codex/state_5.sqlite`; the `sqlite/` child directory is only a compatibility fallback for older/local transitional builds.
- Active DB resolution order should be: explicit UI/CLI override path, `sqlite_home` from Codex config, `CODEX_SQLITE_HOME`, default root `~/.codex/state_5.sqlite`, then legacy fallback `~/.codex/sqlite/state_5.sqlite`. This preserves configured migrations while fixing default macOS detection.
- The history repair UI should describe the default as `~/.codex/state_5.sqlite` and mention automatic `sqlite_home` / `CODEX_SQLITE_HOME` detection, so users do not manually copy the stale sqlite-subdir path into the override field.

## 2026-06-23 MultiRouter Model Modality Alignment

- MultiRouter 不能给新建 route 默认写入 `capabilities: { inputModalities:["text","image"], textOnly:false, supportsReasoning:true }`。这会把 DeepSeek V4 Flash/Pro 等纯文本模型错误标成图文，并且后端 `codex_routing_capabilities_for_model` 会优先信任 route 能力，覆盖模型名纯文本兜底。
- 正确能力来源顺序：route 显式能力 > `modelCatalog.models[]` 条目能力（`inputModalities` / `textOnly` / `supportsImage` / `vision` / `capabilities`）> 保守模型名兜底。未知模型不要默认标成图文，避免多模态/纯文本静默误判。
- DeepSeek Codex 预设的 `deepseek-v4-flash` 和 `deepseek-v4-pro` 应在 `modelCatalog` 中声明 `inputModalities:["text"]`、`textOnly:true`、`supportsImage:false`；MultiRouter 聚合 catalog 要保留这些字段并同步写入 route/catalog 能力。
- Rust `codex_config.rs` 生成 Codex Desktop model catalog 时，也要读取 `modelCatalog.models[]` 的能力声明；只看 route 能力或硬编码模型名会让前端目录和后端投影再次分叉。

## 2026-06-22 Codex MultiRouter User Guide

- 新增用户向说明书 `docs/guides/codex-multirouter-guide-zh.md`，定位为把 Codex Desktop 登录、CCSwitchMulti OAuth 授权、第三方模型源、本地路由映射、MultiRouter 工作台、子 Agent 前 5 候选排序、路由启动、Debug 检查、Codex 重启和历史修复串成完整流程的中文 Markdown。
- 文档只引用仓库已有真实截图：`docs/images/codex-official-auth-preservation/01-codex-app-enhancement-setting.png`、`docs/images/codex-deepseek-routing/01-codex-providers-require-routing.png`、`02-deepseek-codex-routing-form.png`、`03-local-route-codex-takeover.png`。MultiRouter 工作台、子 Agent 排序、状态 Debug、会话管理历史修复等新页面尚无真实截图，文档末尾列出待补路径，后续应补真实 UI 截图，不要伪造。
- 使用规则固化：先登录 Codex Desktop，再在 CCSwitchMulti `设置 → 认证` 完成 ChatGPT/Codex OAuth；额外模型源如 DeepSeek/GLM/本地模型通常要开启 `需要本地路由映射`，在高级选项 `模型映射` 中点击 `获取模型列表` 并配置上下文窗口；MultiRouter 的 `子 Agent 候选模型` 必须手动把目标模型排入前 5 并 `保存排序`；保存/切换/模型目录变化后必须完全退出并重启 Codex Desktop。
- 历史修复说明保持当前产品边界：历史入口在右上角时钟/会话管理页的 `Codex 历史修复`，流程是 `加载历史`、按需全选当前页、`预览修复`、确认计数后 `确认写入`，完成后再次重启 Codex。该功能修复 provider bucket 可见性，不应表述为会话正文丢失修复。
- 主 `README.md` 前部的 CCSwitchMulti 分支说明后新增 `Codex 多路由配置说明书` 小节，直接链接 `docs/guides/codex-multirouter-guide-zh.md`，让首次配置用户先读完整流程而不是只看功能截图。
- 2026-06-22 用户补齐 MultiRouter 教程真实 UI 截图，稳定保存到 `docs/images/codex-multirouter/`：`01-settings-auth-oauth.png`、`02-add-provider-entry.png`、`03-configure-provider-local-routing.png`、`04-fetch-models-context-window.png`、`05-multirouter-entry.png`、`06-create-multirouter.png`、`07-configure-route-rules.png`、`08-save-route-rules.png`、`09-subagent-model-order.png`、`10-enable-routing-settings.png`、`11-debug-entry.png`、`12-13-history-repair-panel.png`、`13-codex-model-picker-validation.png`。这些图对应用户指定的 1-13 步及重启后 Codex 模型候选验证，不要再把这些场景列为待补截图。
- 渲染产物：`docs/images/codex-multirouter-guide/pages/` 保存 Markdown 说明书按页渲染的 PNG，规格为 1440x2400；当前页码包括 `00-overview.png`、`01-flow.png`、`02-step-1.png` 到 `12-step-11.png`、`13-faq.png`、`14-related-docs.png`，并有 `manifest.json` 记录标题和路径。说明书截图变更后必须重新生成这些分页 PNG 和 `output/pdf/codex-multirouter-guide-zh.pdf`。
- 2026-06-23 说明书分页生成流程已抽成仓库内 skill：`skills/markdown-paged-guide/`，包含 `scripts/render_paged_guide.cjs` 和 `scripts/pngs_to_pdf.py`。后续截图型 Markdown 说明书应优先用 `<!-- guide-page: file.png | title -->` 显式分页，统一用 `--max-image-height` 控制全书截图尺寸，再输出 `pages/manifest.json` 与 PDF。当前 MultiRouter 教程已改为 15 页：第一页入门准备，第二页 `总流程速览`，截图统一 `maxImageHeight=500`，避免双截图页底部溢出。

## 2026-06-22 CCSwitchMulti README Xiaohongshu Feedback QR

- GitHub multi README 的活跃源码落点是 `C:\Users\sunda\Documents\LLMservice\cc-switch\README.md`，对应 `fork` remote `https://github.com/BigStrongSun/ccswitchmulti.git`；`C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti` 是固定交付/发布目录，不作为源码 README 修改点。
- README 顶部反馈入口使用仓库内资产 `assets/xiaohongshu-discussion-qr.png`，由用户提供的小红书群截图裁剪出纯二维码区域；README 引用路径保持相对路径 `assets/xiaohongshu-discussion-qr.png`，便于 GitHub 渲染。
- 顶部说明保留两条反馈路径：提交 GitHub Issue，或扫码加入小红书讨论群；二维码来自 2026-06-22 截图，标注有效期至 2026-07-20，后续过期需要替换同名资产并更新有效期文案。
- 纠正：GitHub 默认渲染的 `README.md` 应恢复并保持 `ff29c274 docs(readme): add ccswitchmulti screenshots and scenario` 那版中文 CCSwitchMulti 专属 README，包含“适合谁使用”“功能截图”和 MultiRouter 截图说明；不要用上游 `README_ZH.md` 或英文 `README.md` 覆盖默认首页。
- 配套图片资产必须随该版 README 一起保留：`assets/screenshots/ccswitchmulti/{provider-list,multirouter-status,multirouter-routes,codex-model-picker,usage-statistics}.png` 以及历史赞助图 `assets/partners/logos/lemondata.png`、`assets/partners/logos/ccsub.jpg`。如果只恢复 README 而不恢复这些文件，GitHub README 会出现大面积图片加载失败。

## 2026-06-22 MultiRouter Deletion Flow

- MultiRouter provider 通常就是 Codex 当前 provider；普通 provider 删除链路前端会禁用当前项，后端 `ProviderService::delete` 也会拒绝删除当前 provider，所以工作台必须提供 MultiRouter 专用删除入口。
- 删除当前 Codex MultiRouter 前，先自动切到一个非 MultiRouter 的普通 Codex provider 作为 fallback，再调用原有 `delete_provider`。不要绕过后端当前 provider 保护；保护逻辑仍用于防止误删正在使用的普通 provider。
- 工作台内至少在总览方案卡、路由规则页方案卡、状态页当前方案操作区展示删除按钮。删除动作仍走统一确认框，避免误点。

## 2026-06-22 MultiRouter Routes Compact Layout

- MultiRouter 规则配置页要优先按“同屏操作台”处理：顶部状态只做紧凑状态带，方案栏和规则详情栏不要固定到 360px，主布局应控制在约 300px 侧栏，避免小窗口时规则列表、详情和子 Agent 候选区被挤出屏幕。
- 候选 provider 模型刷新提示必须保留失败可见性，但成功/读取中的状态适合做一行紧凑条目；不要让刷新成功列表单独撑出一个大卡片高度。
- 子 Agent 候选模型面板的右侧候选池需要有 `max-height` + 内部滚动，预览卡片和拖拽项保持低高度；否则候选池会把整个 MultiRouter routes 页向下撑开，截图里顶部和候选区无法一页看全。

## 2026-06-22 MultiRouter Candidate Provider Model Refresh

- MultiRouter 路由规则页不能只消费普通 provider 已经持久化的 `settingsConfig.modelCatalog`，否则新建/切换 MultiRouter 时会停留在旧 GPT fallback，Qwen/DeepSeek/VLLM 等候选普通 provider 不会进入子 Agent 候选。
- 进入 `CodexRouterWorkspacePage` 的 `routes` tab 时，应自动对所有候选普通 Codex provider 调用 `fetch_models_for_config` 读取 `/models`；读取成功后写回该 provider 的 `settingsConfig.modelCatalog.models` 和 `spawnAgentModels`，并同步重建所有引用它的 MultiRouter plan catalog。
- 官方/OAuth provider 没有普通 base_url/API key 时跳过普通 `/models` 读取；普通 provider 缺 base_url、缺 API key、返回空列表或请求失败时，要在路由页和候选 router 卡片上明确提示“获取模型列表失败，请检查当前 provider 配置”。
- MultiRouter 的 `buildModelCatalogForRoutes` 必须按当前 routes 重建 catalog，只复用旧 catalog 的 display/context 元数据，不能无条件保留旧模型；否则取消 GPT route 或改成 VLLM/Qwen route 后，旧 GPT fallback 仍会污染 spawn_agent 前五候选。
- 普通 Codex provider 的“获取模型列表”按钮应把远端模型合并进模型映射表，并在保存时即使不是 `openai_chat` 也持久化非 official provider 的 modelCatalog；保存时空的 `spawnAgentModels` 要从 catalog 前五个自动补齐。

## 2026-06-21 WebDAV Cross-Device Codex Config Contamination

- WebDAV/S3 v2 sync does not upload `~/.codex/config.toml` as a raw file; the protocol uploads `db.sql` plus `skills.zip`.
- The synced SQL snapshot still includes portable and non-portable configuration rows such as `providers`, `mcp_servers`, `settings`, and `proxy_config`. After another device downloads the snapshot, normal CC Switch logic can write those DB rows back into that device's live Codex `~/.codex/config.toml`.
- Therefore cross-user WebDAV sync can effectively contaminate another machine's Codex config with the source machine's absolute paths, for example `notify`, `mcp_servers.*.command`, `mcp_servers.*.args`, local plugin/runtime cache paths, or provider config snippets that contain `C:\Users\<source-user>\...`.
- Do not treat this as Codex randomly generating bad paths. The root cause boundary is CC Switch sync importing machine-local DB values and later live-syncing them to Codex. Safe cross-device sync needs either excluding machine-local rows/fields or adding a per-device reconciliation/sanitization step before writing live configs.

## 2026-06-21 CCSwitchMulti 3.16.3-6 Local Export

- Version bump for the local manual-test build must update all four version surfaces: `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.lock`.
- The local export pipeline for `3.16.3-6` produced Windows artifacts under `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`: `windows\installer\CCSwitchMulti_3.16.3-6_x64-setup.exe`, `windows\portable\CCSwitchMulti_3.16.3-6_x64-portable.zip`, and `windows\raw-exe\CCSwitchMulti_3.16.3-6_x64.exe`.
- Post-commit release hooks can start a background full build immediately after release/version commits. If a manual run hits `scripts\logs\local-release.lock`, inspect `scripts\logs\post-commit-release.log` and wait for cargo/rustc/makensis to exit instead of starting competing builds.

## 2026-06-21 Codex MultiRouter Route Toggle UX

- The MultiRouter route picker has two independent states: candidate membership and route enabled. UI labels must spell this out as `未加入`, `已加入并启用`, or `已加入但停用`; using only `启用/停用` makes users think the checkbox itself is the enabled state.
- In the generic Codex Provider edit form, route row switches must synchronously publish the full `codexRouting` object back to the parent form. Relying only on a child-to-parent effect introduces a one-render race where toggling a route and immediately pressing Save can persist the old enabled value.
- OpenAI/Codex official providers can legitimately have no `modelCatalog`. For route creation/picker display, only those OpenAI-like sources should get GPT/O-series fallback models; do not apply the fallback to every provider whose id starts with `codex-`, or Qwen/DeepSeek catalogs get polluted.
- `RouteCandidatePicker` 的 `selectedIds/enabledIds` 是未保存的本地草稿；同一个多路路由内普通父组件重渲染、provider refetch 或 optimistic plan 刷新时，不能再从 `candidate.route.enabled` 重新初始化，否则用户刚点的 `已启用` 会被旧配置覆盖回 `已停用`。父层 `routingPlans/modelSources` 应保持 memoized，子层候选刷新时只给新出现的 router 应用默认值，已有候选必须保留草稿状态。
- 子 Agent 候选模型面板右侧候选池不要使用固定 `max-h-*` 做滚动高度；它位于和左侧拖拽列表同一行的 grid 中，右侧卡片应 `h-full min-h-0 flex flex-col`，Tabs content 用 `flex-1 min-h-0`，列表自身 `h-full overflow-y-auto`，否则右下角滚动范围会和紫色外框高度不一致。

## 2026-06-21 Codex MultiRouter Picker Persistence

- MultiRouter 工作台的“创建多路路由”不能复用普通 Provider 创建表单；普通表单不会初始化 `settingsConfig.codexRouting`，会把新对象归到普通模型源，导致新建的多路路由在 MultiRouter 列表不可见。
- 正确创建路径是直接 `providersApi.add(nextPlan, "codex", false)` 写入一个带 `settingsConfig.codexRouting.enabled=true`、`routes=[]`、`modelCatalog` 初始目录的 Codex provider，然后打开候选 router 选择器。
- 候选 router 保存时必须把宽松 route 规整成后端可消费结构：稳定 `id`、`enabled`、`targetProviderId`、`match.models/prefixes`、`upstream.apiFormat`、`upstream.auth`，并确保 `defaultRouteId` 指向现有 route。
- 保存候选 routes 时同步重建 `settingsConfig.modelCatalog` 和 `spawnAgentModels`，否则 Codex 选择器/子 Agent 可见模型会与路由规则不一致。
- Tauri/Rust 持久化链路对 `settingsConfig` 是整段 JSON 直通保存：`providersApi.add/update` -> `ProviderService::add/update` -> `db.save_provider` -> SQLite `providers.settings_config`。后端不会裁剪 `codexRouting` 或 `modelCatalog`，本次修复不需要后端 schema 改动。
- MultiRouter provider 自身不是普通 Codex 上游，不应该进入通用 ProviderForm 去填 API Key、API 请求地址、本地模型路由或模型目录。多个 MultiRouter 共享同一套系统投影接管语义：Codex live config 指向 `codex_model_router_v2`、`http://127.0.0.1:15721/v1`、`wire_api=responses` 和 `cc-switch-model-catalog.json`，这些由切换/接管流程和工作台自动维护；用户只编辑方案名称、备注、入口启用、默认 route 和候选 routes。

## 2026-06-16 External OpenAI API Chinese Input Diagnostics

- Current live external Agent API profile was verified read-only from `~/.cc-switch/cc-switch.db`: enabled on `0.0.0.0:15722`, `backendType=provider`, `appType=codex`, `providerId=codex-official`, `defaultModel=gpt-5.5`. This means the reported `/v1/chat/completions` issue goes through External Chat Completions -> synthetic `codex_oauth` provider -> ChatGPT Codex `/backend-api/codex/responses`, not through the normal `15721` MultiRouter route table.
- Source-level UTF-8 chain remains `body.collect().to_bytes()` -> `serde_json::from_slice` -> `serde_json::Value` -> `chat_completions_request_to_codex_responses` -> `serde_json::to_vec` -> reqwest body; no ASCII/Latin-1/GBK conversion was found.
- Real compatibility gap fixed in `src-tauri/src/proxy/providers/openai_compat.rs`: Chat message content parts with Responses-style `type: "input_text"` or `type: "output_text"` were previously dropped because only `type: "text"` was accepted. This can make Codex see only surviving English tokens or references from mixed third-party Agent payloads. The converter now preserves `text`, `input_text`, and `output_text` as Responses text parts.
- Added non-content diagnostics for the external codex-official path: `external_chat_unicode_probe` in `codex-router.log` records text part count, character count, non-ASCII count, question mark count, replacement-character count, and a short hash before forwarding to Codex OAuth. It deliberately does not log prompt text.
- Regression tests added: `chat_request_preserves_chinese_through_codex_responses_conversion`, `chat_request_preserves_responses_style_text_parts`, `v1_chat_completions_preserves_chinese_for_profile_backend`, and `external_codex_unicode_stats_detects_chinese_without_prompt_leak`.

## 2026-06-16 CCSwitchMulti 3.16.2-20 GitHub release

- Published `https://github.com/BigStrongSun/cc-switch/releases/tag/v3.16.2-20` from target commit `b38e0649aeafce68e3c6b300bcb53c22b4edb413` after pushing `feat/codex-local-model-routing` to the fork.
- Uploaded 10 exact assets: Windows setup exe, setup signature, portable zip, raw exe, `CodexHistoryTool_3.16.2-20.zip`, `latest.json`, root `README.md`, Linux/macOS build notes, and `SHA256SUMS-v3.16.2-20.txt`.
- Do not upload the fixed export directory wholesale for this release line: `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti\SHA256SUMS.txt` still includes old version residue such as earlier raw exes. Use the version-specific checksum file for release verification.
- Post-release verification passed: `gh release view v3.16.2-20` reported a non-draft, non-prerelease release with all 10 assets; `git ls-remote --tags fork v3.16.2-20` pointed at the target commit; downloaded release `latest.json` points updater clients at `v3.16.2-20/CCSwitchMulti_3.16.2-20_x64-setup.exe`.

## 2026-06-14 Subagent Visible Model Toolcall Test

- User requested subagent testing for all currently visible Codex models plus toolcall capability.
- Live Codex config at test time used `model_provider = "codex_model_router_v2"` with `model_catalog_json = "cc-switch-model-catalog.json"` and `[model_providers.codex_model_router_v2] base_url = "http://127.0.0.1:15721/v1"`, `wire_api = "responses"`.
- `~/.codex/cc-switch-model-catalog.json` exposed 7 list-visible API-supported slugs with parallel tool calls enabled: `gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.3-codex-spark`, `qwen3.6`, `deepseek-v4-flash`, `deepseek-v4-pro`.
- Subagent + shell toolcall passed for `gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`, `deepseek-v4-flash`, and `deepseek-v4-pro`. Each successful worker ran safe read-only PowerShell checks such as `Get-Location`, `Get-Date`, and `Get-ChildItem`.
- `gpt-5.3-codex-spark` could be spawned but both attempts ended with `You've hit your usage limit. Try again later.`, so model availability/toolcall could not be verified in this run.
- `qwen3.6` first completed with an empty final status, then the explicit retry failed with `unexpected status 502 Bad Gateway` while handling `/responses`; CCSwitch logs showed routing was correct (`route_id=qwen-local`, upstream `https://www.matrixminecraft.cn:24443/vllm/v1/chat/completions`) but the Qwen upstream returned 502 with `<urlopen_error_[Errno_111]_Connection_refused>`. Direct probes to `https://www.matrixminecraft.cn:24443/vllm/v1/models` and `/chat/completions` also returned 502, so the failure boundary is the Qwen vLLM upstream, not local model-catalog visibility or subagent shell toolcall permissions.
- Local router process remained running during the test: PID `46200`, path `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti\windows\raw-exe\CCSwitchMulti_3.16.2-14_x64.exe`, listening on `0.0.0.0:15721`.
- Do not treat unauthenticated `GET http://127.0.0.1:15721/v1/models` returning 401 as proof of model failure; this endpoint requires auth in the current router path.

## 2026-06-14 Codex Desktop Three-Model Runtime Snapshot

- Re-focused the 3-model picker report on the current running Codex Desktop state, not only on provider-id/history cleanup.
- Current live files are valid for MultiRouter: `~/.codex/config.toml` has `model_provider = "cc_switch_codex_router"`, top-level `model_catalog_json = "cc-switch-model-catalog.json"`, and `[model_providers.cc_switch_codex_router]` pointing at `http://127.0.0.1:15721/v1`; both `cc-switch-model-catalog.json` and `models_cache.json` contain the 7 expected slugs.
- A fresh Codex CLI process using the current `~/.codex/config.toml` (`codex debug models`) returns all 7 slugs, proving the generated catalog is parseable by Codex and the model fields are not filtered out by `visibility` / `supported_in_api`.
- The current thread tool description is not reliable proof of a 3-model Desktop picker: Codex hard-caps `spawn_agent` model override descriptions at 5 entries (`MAX_MODEL_OVERRIDES_IN_SPAWN_AGENT_DESCRIPTION = 5`), so DeepSeek can be omitted there even when the static catalog contains it. Use Desktop `model/list` / visible picker evidence for the UI claim.
- Current DB state is valid for MultiRouter: `codex-openai-router` is current, its `modelCatalog` has the 7 expected slugs, and `codexRouting` has enabled OpenAI/Qwen/DeepSeek routes. `codex-router.log` shows real `route_resolved` / `upstream_status` attribution for OpenAI, Qwen, and DeepSeek routes in prior/current runs.
- Codex app-server `model/list` is served from `supported_models(thread_manager)`, and `ThreadManager::new` builds a shared `models_manager` once from the startup `Config`. Later config/catalog writes do not automatically rebuild this manager. If the visible Desktop picker still shows only 3 while fresh `codex debug models` returns 7, the remaining root-cause boundary is the running Desktop app-server/UI model-list snapshot or UI cache, not CCSwitch catalog generation or route configuration.

## 2026-06-13 Codex MultiRouter Stable Bucket Reconciliation

- Re-checked the 3-model Codex Desktop picker issue after the 3.16.2-5 build.
- Live `~/.codex/config.toml` was already in MultiRouter takeover form with top-level `model_catalog_json = "cc-switch-model-catalog.json"`, `base_url = "http://127.0.0.1:15721/v1"`, `wire_api = "responses"`, `requires_openai_auth = false`, and `supports_websockets = false`.
- Live `cc-switch-model-catalog.json` and `models_cache.json` both contained the 7 expected slugs: `gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.3-codex-spark`, `qwen3.6`, `deepseek-v4-flash`, and `deepseek-v4-pro`.
- Codex source archaeology showed `model_catalog_json` is the actual model candidate source; arbitrary non-reserved provider ids do not unlock the picker. Thread/history listing does use the current `model_provider` as its default provider bucket, so changing the MultiRouter id can hide historical sessions.
- Decision: keep `codex_model_router_v2` as the stable runtime MultiRouter provider id, while keeping `cc_switch_codex_router` in legacy/history source lists so older sessions can still migrate. Do not switch back to built-in `openai + openai_base_url` for MultiRouter unless a separate Codex source-level proof requires it.
- Runtime DB note: `codex-openai-router.settings_config.config` may still carry the old `model_provider = "openai"` plus `openai_base_url` persisted shape, but takeover code normalizes it to the stable local provider table. Future cleanup can normalize the stored provider config too, but the live candidate source is the generated catalog pointer.

## 2026-06-13 Codex MultiRouter Candidate Bucket Fix

- User reported the current CCSwitchMulti build still showed only three OpenAI candidates in Codex Desktop, while the older 2026-06-08 CCSwitchMulti build showed the full MultiRouter list.
- Code/DB archaeology:
  - 2026-06-08 working backups used `model_provider = "cc_switch_codex_router"` plus top-level `model_catalog_json = "cc-switch-model-catalog.json"` and `[model_providers.cc_switch_codex_router]`.
  - The working path was the static Codex `model_catalog_json` file with 7 router model slugs, not `models_cache.json` alone and not the later `openai + openai_base_url` experiment.
  - The current local DB had drifted to `model_provider = "openai"` with `openai_base_url = "http://127.0.0.1:15721/v1"` in `codex-openai-router.settings_config.config`, which risks pushing the picker back into Codex's built-in OpenAI provider semantics.
- Fix:
  - `src-tauri/src/codex_config.rs` now sets `CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID` to `cc_switch_codex_router`.
  - `src-tauri/src/services/proxy.rs` keeps normal third-party Codex providers on `custom`, but MultiRouter takeover writes the 2026-06-08 router bucket, removes `openai_base_url`, and keeps `supports_websockets = false`.
  - `src-tauri/src/codex_history_migration.rs` treats `cc_switch_codex_router` as a known router/openai-history source so history sync does not split buckets.
  - `src-tauri/src/services/provider/mod.rs` regression test now starts from the drifted `openai + openai_base_url` persisted config and asserts the live config is normalized to `cc_switch_codex_router` with 7 catalog/cache models.
- Verification passed:
  - `cargo test --manifest-path src-tauri\Cargo.toml switching_codex_router_provider_auto_enables_dedicated_local_takeover --lib -- --nocapture`
  - `cargo test --manifest-path src-tauri\Cargo.toml history --lib`
  - `cargo test --manifest-path src-tauri\Cargo.toml --lib codex`
  - `cargo fmt --manifest-path src-tauri\Cargo.toml --check`
  - `cargo check --manifest-path src-tauri\Cargo.toml` (only pre-existing warnings in `commands/misc.rs`)

## 2026-06-11 Codex Windows App Upgrade Strategy

- User reported Codex CLI update failure from the CC Switch settings page: current `0.137.0`, latest `0.139.0`, toast stack included `aws_lc_0_39_0_jent_entropy_switch_notime...`.
- Local diagnosis:
  - Default `codex` resolves to `C:\Users\sunda\AppData\Local\OpenAI\Codex\bin\codex.exe`.
  - Another Codex executable exists under `C:\Program Files\WindowsApps\OpenAI.Codex_26.608.1337.0_x64__2p2nqsd0c76g0\app\resources\codex.exe`.
  - `codex --version` is `codex-cli 0.137.0`.
  - `codex update` says it cannot detect the installation method.
  - `npm view @openai/codex version` is `0.139.0`, but `winget upgrade --id 9PLM9XGG6VKS --source msstore` reports no available Store upgrade.
- Root cause: the previous Windows lifecycle updater treated Codex App/MSIX launcher paths as ordinary system/npm installs and could build `codex update || npm i -g @openai/codex@latest`, mixing the Codex App runtime with the user's WinGet Node/npm.
- Fix in `src-tauri/src/commands/misc.rs`:
  - Classify `AppData\Local\OpenAI\Codex`, `WindowsApps\OpenAI.Codex_...`, and `Microsoft\WindowsApps\codex.exe` paths as `codex-app`.
  - For Codex App/MSIX installs, generate a Store package update command with `winget upgrade --id 9PLM9XGG6VKS --source msstore --accept-source-agreements --accept-package-agreements`.
  - Do not attach npm fallback for this install source.
  - If multiple Codex entries are detected and no default install can be selected, any Codex App/MSIX entry forces the Store update command instead of the static `codex update || npm ...` fallback.
- Regression coverage:
  - `codex_windows_app_uses_ms_store_upgrade_without_npm_fallback`.
  - `ambiguous_codex_app_install_uses_ms_store_upgrade`.
  - `windows_codex_app_is_identified`.
  - Validation passed: `cargo test --manifest-path src-tauri\Cargo.toml anchored_upgrade_windows --lib`, `cargo test --manifest-path src-tauri\Cargo.toml install_source_classification --lib`, `cargo fmt --manifest-path src-tauri\Cargo.toml --check`, `cargo check --manifest-path src-tauri\Cargo.toml`.

## 2026-06-08 Router UI/Save Logic Fix

- Latest user symptom: after launching the portable build and selecting `OpenAI Multi-Model Router`, Codex Desktop still only showed OpenAI/GPT candidates and lost `gpt-5.3-codex-spark`, DeepSeek, and Qwen. The CC Switch list also showed `OpenAI Multi-Model Router` with the `不支持路由` badge.
- Multi-agent assessment: this was a narrow local state + UI/save-path diagnosis, so the main agent handled it directly instead of spawning subagents. Verification was done through process checks, DB inspection, typecheck, and packaging.
- Live process check:
  - Running process was PID `48844`, started `2026-06-08 20:39:21`.
  - Path: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target-router-fix-20260608_172503\release\bundle\portable\cc-switch.exe`.
  - This was the earlier 17:34 router-candidate portable, not the newer UI/save-logic fixed build below.
- Local DB hotfix:
  - Backup directory: `C:\Users\sunda\.cc-switch\backups\codex-router-category-fix-20260608_205059`.
  - `codex-openai-router.category` was corrected from `official` to `aggregator`.
  - Current provider was left as `codex-official`; no runtime switch away from the user's backup/official line was performed.
- Current Codex config check:
  - `C:\Users\sunda\.codex\config.toml` currently has no `model_provider`, `model_catalog_json`, local `base_url`, or `127.0.0.1` router/proxy lines, so Codex Desktop is still effectively on the backup/official config.
  - `C:\Users\sunda\.codex\cc-switch-model-catalog.json` exists and contains 7 model slugs: `gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.3-codex-spark`, `deepseek-v4-flash`, `deepseek-v4-pro`, and `qwen3.6`.
  - Therefore a missing CodexSpark/DeepSeek/Qwen dropdown after this state means the router takeover was not active, not that the catalog file was absent.
- Root causes:
  - `src/components/providers/ProviderCard.tsx` treated every Codex `official` category provider as `不支持路由`, even when `settings_config.codexRouting` existed. A router provider with official OAuth routes must still be treated as proxy-routed.
  - `src/hooks/useProviderActions.ts` only required the proxy for non-official providers. A Codex router with `codexRouting` now also requires the local proxy even when route auth uses managed official OAuth.
  - `src/components/providers/forms/ProviderForm.tsx` skipped `modelCatalog` and `codexRouting` persistence for category `official`, and only saved the model catalog for `openai_chat`. The router's outer API is `openai_responses`, so editing/saving it could wipe the generated catalog and routes.
- Code fix:
  - `ProviderCard.tsx` now detects `settings_config.codexRouting`, marks such Codex providers as needing routing, and suppresses the false `不支持路由` badge.
  - `useProviderActions.ts` now treats Codex router providers as local-proxy-required providers and allows them during proxy takeover.
  - `ProviderForm.tsx` now preserves `modelCatalog` and `codexRouting` when routing is enabled or routes exist, including router providers whose outer API format is `openai_responses`.
- Verification:
  - `pnpm typecheck` passed.
  - `pnpm tauri build --bundles nsis --config "$env:TEMP\cc-switch-tauri-no-updater.json"` passed.
- Latest UI/save-logic fixed artifacts:
  - Portable exe: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target-router-ui-fix-20260608_210732\release\bundle\portable\cc-switch.exe`
    - SHA256 `4D3E0A7EC297901CEEAB972B3B70018521F0052077AEB6062F4468BE2B6F036A`
  - Portable zip: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target-router-ui-fix-20260608_210732\release\bundle\portable\CC Switch_3.16.1_x64-portable.zip`
    - SHA256 `1D7338E7F137D5CA1888F3A966F8877DA26CB8F3CEE8A87324075F0EE53CDAC7`
  - NSIS installer: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target-router-fix-20260608_172503\release\bundle\nsis\CC Switch_3.16.1_x64-setup.exe`
    - SHA256 `A1194B9A55BB2478BA182FAB1A6C7FF9AACA6DEED450A4A4662947099D5C298A`
- Architecture clarification:
  - `OpenAI Multi-Model Router` is not merely upstream CC Switch's native provider switcher, and it is not an external script. It depends on the local Codex multi-model routing patch now present in this repo.
  - Native CC Switch routing/proxy takeover can redirect Codex to one selected provider, but by itself it does not create a single Codex Desktop model dropdown containing OpenAI, CodexSpark, DeepSeek, and Qwen candidates.
  - The patched path has three required layers: `settings_config.modelCatalog` projects `~/.codex/cc-switch-model-catalog.json` so Codex can display all candidates; `settings_config.codexRouting` stores model-to-upstream routes; the Rust local proxy resolves the request `model` via `resolve_codex_model_routed_provider` and converts Responses to Chat where needed.
  - Therefore the multi-model dropdown requires CC Switch local proxy/takeover plus the patched `modelCatalog`/`codexRouting` implementation. Switching ordinary providers alone is not enough.

## 2026-06-08 Router Candidate/Timeout Fix Package

- Root cause found in the local user DB:
  - `codex-openai-router.settings_config.modelCatalog.models` only contained 4 OpenAI models, so Codex candidate model UI could not show DeepSeek/Qwen.
  - `codex-openai-router.settings_config.codexRouting` was missing, so even a selected DeepSeek/Qwen model would not have a route.
  - Code gap: `src-tauri/src/services/provider/live.rs::restore_live_settings_for_provider_backfill` preserved DB-only `modelCatalog` but not DB-only `codexRouting`; switch-away backfill from Live could wipe the router route table because Live `config.toml` cannot represent it.
- Code fix:
  - `src-tauri/src/services/provider/live.rs` now preserves both `modelCatalog` and `codexRouting` during Codex backfill.
  - Regression test added: `codex_switch_backfill_preserves_stored_codex_routing_when_live_lacks_it`.
- Local DB fix:
  - Backup: `C:\Users\sunda\.cc-switch\backups\codex-router-multimodel-fix-20260608_172503\cc-switch.db.before`.
  - Current provider was left as `codex-official`; no official/backup runtime switch was performed.
  - Router catalog models now include `gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.3-codex-spark`, `deepseek-v4-flash`, `deepseek-v4-pro`, and `qwen3.6`.
  - Router routes:
    - `openai-official`: `gpt-*` -> `https://chatgpt.com/backend-api/codex`, `openai_responses`, `managed_codex_oauth`.
    - `deepseek`: `deepseek-*` -> `https://api.deepseek.com`, `openai_chat`. DeepSeek key is currently empty, so the candidate appears but requests need a key before success.
    - `qwen-local`: `qwen3.6` -> `https://www.matrixminecraft.cn:24443/vllm/v1`, `openai_chat`, `apiKey=vllm-local`.
- Verification:
  - `cargo test codex_switch_backfill --manifest-path src-tauri\Cargo.toml`
  - `cargo test codex_route --manifest-path src-tauri\Cargo.toml`
  - `cargo fmt --manifest-path src-tauri\Cargo.toml --check`
  - `pnpm typecheck`
  - Qwen upstream `/v1/models` returned `qwen3.6`.
- Latest artifacts were built into an isolated target to avoid overwriting the currently running old portable instance:
  - Target dir: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target-router-fix-20260608_172503`.
  - Portable zip: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target-router-fix-20260608_172503\release\bundle\portable\CC Switch_3.16.1_x64-portable.zip`
    - SHA256 `41D9FA3DB194F299F79772E5BABFF72D79AE9262332DD98142E90DDE802BCFDB`
  - Portable exe: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target-router-fix-20260608_172503\release\bundle\portable\cc-switch.exe`
    - SHA256 `9D921B3122CB8FE436974F10DF8BAF1ABF2628812D66E12A7A3A7070727B9B26`
  - NSIS installer: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target-router-fix-20260608_172503\release\bundle\nsis\CC Switch_3.16.1_x64-setup.exe`
    - SHA256 `EC9936E4987985ABA8A2B066831AE1D853FD1BF972FE32CE38590615622FA146`
  - MSI: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target-router-fix-20260608_172503\release\bundle\msi\CC Switch_3.16.1_x64_en-US.msi`
    - SHA256 `38D4E2F7AAC10F27801E5BBDAEFB8B7DB6AE3D33658020DE27ACFA2E155C32D8`
- Packaging note:
  - `pnpm tauri build` produced the release exe, NSIS, and MSI but exited 1 at updater artifact signing because `TAURI_SIGNING_PRIVATE_KEY` is not set. The portable zip was manually generated from the new release exe, matching the existing local portable maintenance pattern.
  - To test the new portable build, close the old local modified CC Switch window first; the single-instance plugin can otherwise bring the old process to front. Codex official does not need to be stopped.

## 2026-06-08 DeepSeek Key Local Configuration

- User provided a DeepSeek key and asked to configure it locally. Do not commit or document the full key; only use masked form `sk-b931...b870` in notes.
- Backup directory before the write: `C:\Users\sunda\.cc-switch\backups\codex-deepseek-key-20260608_203307`.
- Updated local DB fields:
  - `codex-deepseek.settings_config.auth.OPENAI_API_KEY`.
  - `codex-openai-router.settings_config.auth.OPENAI_API_KEY`.
  - `codex-openai-router.settings_config.codexRouting.routes[id=deepseek].upstream.apiKey`.
- Current provider was left as `codex-official`; no switch/takeover was performed.
- Lightweight verification against `https://api.deepseek.com/v1/models` succeeded and returned `deepseek-v4-flash` and `deepseek-v4-pro`.

## 2026-06-08 Packaging And Maintenance

- Current local build artifacts:
  - NSIS installer: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target\release\bundle\nsis\CC Switch_3.16.1_x64-setup.exe`
  - Portable zip: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target\release\bundle\portable\CC Switch_3.16.1_x64-portable.zip`
  - Portable exe: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target\release\bundle\portable\cc-switch.exe`
  - Raw release exe: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target\release\cc-switch.exe`
- Local verification before packaging:
  - `pnpm run typecheck`
  - `cargo test codex --lib` from `src-tauri`
- Recommended local packaging command:
  - Create temp config `C:\Users\sunda\AppData\Local\Temp\cc-switch-tauri-no-updater.json` with `{"bundle":{"createUpdaterArtifacts":false}}`.
  - Run `pnpm tauri build --bundles nsis --config "$env:TEMP\cc-switch-tauri-no-updater.json"`.
- Do not use plain `pnpm run build` as the final local handoff command unless `TAURI_SIGNING_PRIVATE_KEY` is available and MSI/WiX is intentionally required.
  - Current `tauri.conf.json` has updater public key plus `createUpdaterArtifacts=true`, so local builds without a private key fail after bundle generation.
  - Full target builds also invoke MSI/WiX; `light.exe` has previously made the command exit 1 even when `cc-switch.exe` and installer files were produced.
  - Treat the NSIS no-updater command above as the reliable local packaging path.
- Portable package maintenance:
  - Copy `src-tauri\target\release\cc-switch.exe` to `src-tauri\target\release\bundle\portable\cc-switch.exe`.
  - Zip only that exe into `CC Switch_3.16.1_x64-portable.zip`.
  - Portable and installed builds share user data in `~/.cc-switch` and `~/.codex`; do not run them concurrently with the official production app.
- Official production app safety:
  - Do not stop or restart the installed official process during local diagnosis/build work.
  - Last verified official process path: `C:\Users\sunda\AppData\Local\Programs\CC Switch\cc-switch.exe`.

## 2026-06-08 Local Codex Provider Cleanup

- User restored historical `~/.cc-switch` config and explicitly said future cleanup must not use that DB content as a template.
- Canonical Codex provider writes should follow latest repo schema:
  - Pure official fallback: `codex-official`, `settings_config={"auth":{},"config":""}`, no `model_provider`, no `base_url`, no `model_catalog_json`, no `codexRouting`.
  - New router providers must use `settings_config.codexRouting`; legacy `codexModelRoutes` / `modelRoutes` are read-only compatibility paths.
  - `meta.apiFormat` and route `upstream.apiFormat` are the explicit API-format source for proxy conversion.
  - Chat-compatible DeepSeek/Qwen providers should use `meta.apiFormat="openai_chat"` and TOML `wire_api="chat"`.
  - Do not put router TOML, `model_catalog_json`, or `127.0.0.1:15721/15722` into `settings.common_config_codex`.
- Local machine cleanup performed 2026-06-08 15:10:
  - Kept only `codex-official`, `codex-openai-router`, `codex-qwen-local`, and `codex-deepseek`.
  - Set `currentProviderCodex="codex-official"`, `enableLocalProxy=false`, cleared `common_config_codex`, disabled Codex takeover flags, and removed Codex `proxy_live_backup`.
  - Backup path: `C:\Users\sunda\.cc-switch\backups\codex-clean-20260608_150944`.

## 2026-06-08 Codex Local Model Routing

### Product Direction Update

- User clarified that the main UI should be a separate Model Router workspace, not only an embedded route editor inside `CodexFormFields`.
- Desired flow: configure or import multiple model sources first, then select sources and merge them into one router provider that Codex reaches through CC Switch local proxy.
- Prototype artifacts:
  - `docs/prototypes/codex-router-workspace-prototype.html`
  - `docs/guides/codex-model-router-workspace-prototype.md`
- Existing `CodexFormFields` Local model routing editor should be treated as an advanced/generated-config surface unless the prototype review decides otherwise.
- Prototype v2 decision: the Model Router workspace must follow the existing CCSwitch header/AppSwitcher/provider-card style, not a generic SaaS dashboard or left-sidebar layout.
- Prototype v2 entry/exit rules: users can enter from the Codex Provider list, the Codex provider form, or Universal Provider; after publish they return to the Codex Provider list with the generated router provider highlighted.
- Prototype v2 source library rules: source setup must guide provider creation/import, base URL/auth/API format setup, connection test, model fetch, capability query, manual capability edit, and real route testing.
- Prototype v2 catalog rules: one provider/source may expose many upstream models, so UI must support fetched model lists and user-controlled visible models before writing Codex model catalog.
- Prototype v2 publish rule: route success must be tested through the CC Switch Rust local proxy before final publish; static config validation alone is not enough.
- Proposed UI component split for real implementation: `src/components/codex-router/ModelRouterWorkspace.tsx`, `RouterSourceLibrary.tsx`, `RouterSourceEditorDialog.tsx`, `RouterModelCatalogPanel.tsx`, `RouterSummaryPanel.tsx`, `RouteTestPanel.tsx`, and a draft/publish adapter.
- Prototype v3 visual correction: the static prototype must use CCSwitch's dark desktop-app style, wide 16:10 window proportions, top toolbar/app switcher, orange circular add button, blue active borders, and long horizontal provider cards.
- Prototype v3 information architecture: split the router workspace into multiple pages (`Overview`, `Sources`, `Models`, `Routes`, `Test & Publish`) using left-side step navigation; do not stack all router content into one vertical long page.

### Branch And Sync

- Feature branch: `feat/codex-local-model-routing`.
- Created from latest `origin/main` after stashing the old local WIP.
- Protective stash kept for now: `stash@{0}` named `wip-codex-local-routing-before-upstream-sync-20260608-005258`.
- Untracked `run-release-and-check.bat` existed after applying the stash; do not delete it unless the owner confirms it is disposable.

### Canonical Config

- New route config lives under `settings_config.codexRouting`.
- Shape:
  - `enabled`: enables/disables the resolver.
  - `defaultRouteId`: fallback route id when no exact/prefix rule matches.
  - `routes[]`: user-defined route list.
- Route fields:
  - `id`, `label`, `enabled`.
  - `match.models` for exact model ids.
  - `match.prefixes` for model id prefixes.
  - `upstream.baseUrl`.
  - `upstream.apiFormat`: `openai_responses`, `openai_chat`, or `openai_messages`.
  - `upstream.auth.source`: first version supports `provider_config`, `managed_codex_oauth`, and `managed_account`.
  - `upstream.apiKey` for provider-config key material when needed.
  - `upstream.modelMap` for Codex model id to upstream model id mapping.
  - `capabilities.textOnly`, `capabilities.inputModalities`, `capabilities.supportsReasoning`.
- Legacy fields `settings_config.codexModelRoutes` and `settings_config.modelRoutes` are read-only fallbacks. The UI may load them and save back to `codexRouting`.
- `reuse_provider:<id>` is intentionally not supported in the first version.

### Rust Entry Points

- Route resolver and effective provider construction:
  - `src-tauri/src/proxy/providers/codex.rs`
  - Main entry: `resolve_codex_model_routed_provider`.
  - Effective routed provider id format: `{outer_provider_id}::route::{route_id}`.
  - Managed Codex OAuth routes must remove inherited provider `auth` / `apiKey`; otherwise stale Bearer keys can override the managed account chain.
- Forwarding and protocol selection:
  - `src-tauri/src/proxy/forwarder.rs`
  - Reuses existing forwarder flow after route resolution.
  - Supports Responses passthrough, Responses -> Chat, and Responses -> Messages endpoint handling.
- Responses to Chat conversion:
  - `src-tauri/src/proxy/providers/transform_codex_chat.rs`
  - Text-only route capability prevents emitting Chat `image_url` blocks.
- Model catalog capability generation:
  - `src-tauri/src/codex_config.rs`
  - Route capabilities override hardcoded text-only model-name fallbacks.

### Frontend Entry Points

- Shared types:
  - `src/types.ts`
  - `CodexRoutingConfig`, `CodexRoutingRoute`, `CodexRoutingAuth`, `CodexRoutingCapabilities`.
- Codex config state:
  - `src/components/providers/forms/hooks/useCodexConfigState.ts`
  - Reads `codexRouting`; migrates `codexModelRoutes` / `modelRoutes` into UI state.
- Provider save path:
  - `src/components/providers/forms/ProviderForm.tsx`
  - Saves `settings_config.codexRouting` when routing is enabled or routes exist.
- Codex UI:
  - `src/components/providers/forms/CodexFormFields.tsx`
  - Adds **Local model routing** controls as a route summary list plus an edit dialog for match rules, upstream API format, auth, mapping, and capabilities.
  - The Local model routing panel is independent of endpoint speed-test visibility; it should show whenever the Codex form has routing state.
  - Switching a route from `provider_config` to a managed auth source should clear route `apiKey` so stale keys are not persisted.
- i18n keys live under `codexConfig` in:
  - `src/i18n/locales/en.json`
  - `src/i18n/locales/zh.json`
  - `src/i18n/locales/zh-TW.json`
  - `src/i18n/locales/ja.json`

### Docs

- Existing DeepSeek guide paths are now generic Codex Local Model Routing guides:
  - `docs/guides/codex-deepseek-routing-guide-en.md`
  - `docs/guides/codex-deepseek-routing-guide-zh.md`
  - `docs/guides/codex-deepseek-routing-guide-ja.md`
- The filenames still contain `deepseek` for link compatibility, but the content is generic and UTF-8.

### Validation Commands Used

- Rust focused validation:
  - `cargo fmt`
  - `cargo test codex --lib`
- Frontend type validation:
  - `pnpm run typecheck`
- Frontend route UI validation:
  - `pnpm vitest run tests/components/CodexFormFields.test.tsx tests/components/ProviderForm.codexCatalog.test.ts`
- Renderer build validation:
  - `pnpm run build:renderer`

### Maintenance Notes

- When fixing route bugs, update this file if the schema, resolver behavior, or capability semantics change.
- If text-only/image behavior changes, update both catalog generation and Responses -> Chat conversion tests.
- Keep Codex connected to the CC Switch Rust local proxy for this design; route selection should depend on `body.model`, not the GUI's currently selected upstream provider.

## 2026-06-08 Codex v2 DeepSeek v4 Local Proxy Fix

- Canonical user-facing model spelling for this workspace is `deepseekv4`, while configured aliases may include `deepseek-v4-pro`, `deepseek-v4-flash`, or display names such as `DeepSeek V4 Pro`.
- The intended Codex path is still v2 through the CC Switch Rust local proxy: Codex sends `/responses` to `http://127.0.0.1:<proxy>/v1`, CC Switch selects a route, then translates to the route upstream format when needed.
- The DeepSeek v4 failure was not caused by the old user script. It came from the built-in Rust Responses -> Chat conversion emitting Chat `content[]` image blocks for a text-only upstream. DeepSeek rejected this with `unknown variant image_url, expected text`.
- Text-only detection for DeepSeek v4 must use compact model-id normalization so `deepseekv4`, `deepseek-v4-*`, and spaced display aliases are all treated the same.
- Keep DeepSeek v4 text-only behavior aligned across `src-tauri/src/proxy/providers/transform_codex_chat.rs`, `src-tauri/src/codex_config.rs`, and `src-tauri/src/proxy/media_sanitizer.rs`.
- GUI route creation should not persist default `capabilities: { textOnly:false, inputModalities:["text","image"], supportsReasoning:false }` for new routes, because that can create a false explicit image-capability override.
- Route-level `codexChatReasoning.minOutputTokens` is supported for Chat upstreams that need a larger minimum output budget to avoid reasoning consuming tiny Codex probe responses.
- Validation commands used for this fix: `cargo fmt`, `cargo test transform_codex_chat --lib`, `cargo test media_sanitizer --lib`, `cargo test codex_model_catalog --lib`, `cargo test codex --lib`, and `node node_modules\typescript\bin\tsc --noEmit`.

## 2026-06-08 Codex Multi-Model Router Detail Fix

- The working router provider is the patched CC Switch Rust local proxy path, not native provider switching alone. Codex connects to CC Switch, the proxy reads `body.model`, resolves `settings_config.codexRouting`, and forwards to OpenAI official, DeepSeek, or Qwen.
- Stable Codex history bucket for this local router is `codex_model_router_v2`. Avoid reintroducing `cc_switch_codex_router`; it splits Codex Desktop history into another provider bucket. On this machine, old `codex_model_router` rows were merged into `codex_model_router_v2` with backup at `%USERPROFILE%\.codex\backups\router-provider-v2-merge-20260608_225952`.
- Router provider DB config currently uses `model_provider = "codex_model_router_v2"` with `[model_providers.codex_model_router_v2] base_url = "http://127.0.0.1:15721/v1"` and `wire_api = "responses"`.
- Route/candidate catalog currently exposes 7 models: `gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.3-codex-spark`, `deepseek-v4-flash`, `deepseek-v4-pro`, and `qwen3.6`.
- `src-tauri/src/codex_config.rs` must preserve `additional_speed_tiers` and `service_tiers` for OpenAI official `gpt-*` entries, except `codex-spark`; third-party/local models should still clear these fields so the UI does not show official service tiers on DeepSeek/Qwen.
- Existing on-disk catalog was manually refreshed after the code fix; old file backup is `%USERPROFILE%\.codex\backups\catalog-speed-tiers-20260608_231320`.
- `src-tauri/src/proxy/codex_router_log.rs` writes compact diagnostics to `%USERPROFILE%\.cc-switch\logs\codex-router.log`. It logs route, auth, request preparation, upstream send/status/error, and response readiness by trace id without raw prompt, token, header, or SSE content.
- `src-tauri/src/lib.rs` should not delete `%USERPROFILE%\.cc-switch\logs\cc-switch.log` on startup; early router cutover errors must survive restart.
- Avoid raw request/SSE logs in normal Debug/Trace. `forwarder.rs` should log request bytes plus body hash; `response_processor.rs` should only parse SSE when usage collection requires it.

## 2026-06-09 CCSwitchMulti Config Preservation And Packaging

- Current local modified build is branded `CCSwitchMulti` to distinguish it from the official `CC Switch` binary. The app still uses the existing `.cc-switch` data directory so provider DB/config history remains shared; do not rename the config directory unless deliberately doing a clean-room install.
- Package identity for the modified installer is `com.ccswitchmulti.desktop`; deep-link scheme is `ccswitchmulti`. This prevents the local installer from being treated as the same app identity as official `com.ccswitch.desktop`.
- MSI packaging rejects prerelease ids like `multi.1`; use numeric prerelease `3.16.1-1` for this local build line. The visible distinction comes from `productName = "CCSwitchMulti"` plus the numeric local build suffix.
- Current delivery directory: `src-tauri/target-ccswitchmulti-20260609_001033/`.
  - Portable zip: `CCSwitchMulti_3.16.1-1_x64-portable.zip`.
  - Portable exe: `CCSwitchMulti.exe`.
  - NSIS installer: `CCSwitchMulti_3.16.1-1_x64-setup.exe`.
  - MSI installer: `CCSwitchMulti_3.16.1-1_x64_en-US.msi` copied from `src-tauri/target/release/wix/x64/output.msi` after Tauri's MSI final copy failed.
- Build cleanup on 2026-06-09 removed stale local modified targets `src-tauri/target-router-fix-20260608_172503`, `src-tauri/target-router-ui-fix-20260608_210732`, and `src-tauri/target-router-detail-fix-20260608_230505`, the default build cache `src-tauri/target`, and the old root release artifacts `cc-switch-release` / `cc-switch-release.zip`. A stale portable process from `target-router-detail-fix-20260608_230505` had to be stopped to unlock that old directory; the official backup instance was not stopped.
- After cleanup, only `src-tauri/target-ccswitchmulti-20260609_001033` should be used for current delivery artifacts. Do not hand users any old `target-router-*`, default `target`, or root `cc-switch-release*` artifact paths.
- In this environment `pnpm` may be absent from PATH, while local `node_modules` exists. `tauri.conf.json` now uses `node ./node_modules/vite/bin/vite.js build` for `beforeBuildCommand`; frontend validation can use `.\node_modules\.bin\tsc.CMD --noEmit`.
- Tauri NSIS bundling can return exit code 1 after successfully producing setup.exe when updater signing has a public key but no `TAURI_SIGNING_PRIVATE_KEY`. Treat the generated setup file as usable if it exists and hashes cleanly; record this caveat in handoff.
- Codex history reality on this machine: `state_5.sqlite` had 445 threads during the 2026-06-09 check, with 432 under `codex_model_router_v2` and only 13 under `openai`. Full history is not mostly in `openai`.
- Codex `thread/list` defaults to filtering by current `model_provider` when `modelProviders` is omitted. Passing `modelProviders: []` means no provider filter. Optional `cwd` filters are exact-path filters and can make history appear limited to the current workspaces.
- Do not create another router provider id. Keep router provider config at `model_provider = "codex_model_router_v2"` so the Codex Desktop history bucket stays stable.
- Provider switching must never write provider `config.toml` snapshots verbatim over the current live Codex config. `src-tauri/src/codex_config.rs` now merges provider config with live config: provider top-level scalar fields and `[model_providers.<active-id>]` override, while live `[features]`, `[desktop]`, `[memories]`, `[projects]`, `[mcp_servers]`, plugins, and other user tables are preserved.
- Common config snippets still need to add missing table entries. The merge behavior is "live wins on conflicts, provider/common config fills missing table keys." This preserves user MCP entries while allowing CC Switch common config to add new MCP servers.
- Proxy takeover placeholder branches in `src-tauri/src/services/proxy.rs` must also merge before `write_codex_live_config_atomic`; otherwise switching router during takeover can clear context-window display, memories, MCP, and project trust.
- Validation for this fix used `.\node_modules\.bin\tsc.CMD --noEmit` and `cargo test codex --lib` (318 passed).

## 2026-06-09 CCSwitchMulti History Visibility And Router Preservation Fix

- Live official state after the 2026-06-09 01:20 check: `codex-official` is current in `~/.cc-switch/cc-switch.db`, `currentProviderCodex` is `codex-official`, Codex proxy flags are disabled, and `~/.codex/config.toml` has no local router/proxy lines. If the UI still feels like it did not switch back, first distinguish live config from Codex history filtering.
- Runtime DB repair restored `codex-openai-router.settings_config.codexRouting` with three routes:
  - `openai-official`: `gpt-*` via `https://chatgpt.com/backend-api/codex`, `openai_responses`, `managed_codex_oauth`.
  - `deepseek`: `deepseek-v4-flash` / `deepseek-v4-pro` via `https://api.deepseek.com`, `openai_chat`, provider_config key.
  - `qwen-local`: `qwen3.6` via `https://www.matrixminecraft.cn:24443/vllm/v1`, `openai_chat`, `minOutputTokens=2048`.
- Backup before runtime repair: `%USERPROFILE%\.cc-switch\backups\codex-history-official-router-fix-20260609_012627`.
- `src/components/providers/EditProviderDialog.tsx` now preserves both DB-private Codex fields, `modelCatalog` and `codexRouting`, when editing the current provider after reading live settings. This prevents saving a current router provider from erasing its route table.
- `src-tauri/src/codex_config.rs` now preserves OpenAI speed/service tiers only for `gpt-5.5` and `gpt-5.4`. `gpt-5.4-mini`, `gpt-5.3-codex-spark`, DeepSeek, Qwen, and other generated catalog entries must have empty `additional_speed_tiers` and `service_tiers`.
- Current on-disk `~/.codex/cc-switch-model-catalog.json` was repaired to match that rule: `gpt-5.5` and `gpt-5.4` keep `fast/priority`; mini, spark, DeepSeek, and Qwen have no service tiers.
- History visibility analysis from the read-only subagent:
  - `state_5.sqlite` has 448 threads. `session_index.jsonl` has 426 unique ids; sqlite has 24 ids not in the jsonl index and jsonl has 2 ids not in sqlite.
  - Provider buckets: `codex_model_router_v2=433`, `openai=15`.
  - Source buckets: `vscode=223`, `exec=26`, `subagent=199`; archived threads total 142.
  - Visible history is mostly a view/filtering problem, not data loss. Default `thread/list` behavior filters by active provider when `modelProviders` is omitted, hides non-interactive sources when `sourceKinds` is omitted/empty, excludes archived items, applies exact `cwd` filters, and paginates.
  - To surface hidden history safely, prefer fixing the query/view: pass `modelProviders: []`, include non-interactive `sourceKinds`, avoid default exact `cwd`, expose archived separately, and page through `nextCursor`. Do not rewrite sqlite buckets just to make old sessions visible.
- Latest packaged delivery for this fix:
  - Directory: `src-tauri/target-ccswitchmulti-historyfix-20260609_013447/`.
  - Portable exe: `CCSwitchMulti.exe` SHA256 `909933223A40D6AECA5396F3D1B2A2104C22ECD86EF68DB7DF5B493B1D1DD65F`.
  - Portable zip: `CCSwitchMulti_3.16.1-1_x64-portable.zip` SHA256 `8985C3F5B5C8D5C54C8DA70E4B3D5D1E444C25454794D9DDD7B959FCDD4111FA`.
  - NSIS installer: `CCSwitchMulti_3.16.1-1_x64-setup.exe` SHA256 `3E7C668881D7B7E0EB61F8D754D95971A59046FA6C7EB8C07260B3E11CB2D3CE`.
  - MSI installer: `CCSwitchMulti_3.16.1-1_x64_en-US.msi` SHA256 `D15EAC130332CA0717001630E334C32D2FB9895A14BE47D23866612908906DE7`.
- Validation: `vitest` for `EditProviderDialog` and `CodexFormFields` passed 5 tests; `cargo test codex_model_catalog --lib` passed 5 tests; `.\node_modules\.bin\tsc.CMD --noEmit`, `cargo fmt --check`, and `cargo test codex --lib` passed 319 tests; Tauri no-updater build succeeded.
- The older `src-tauri/target-ccswitchmulti-20260609_001033/CCSwitchMulti.exe` was still running during packaging. Do not delete that old directory until the old process is closed or replaced by the new build.

## 2026-06-09 CCSwitchMulti Rootfix For Codex Official Fallback And Router Pollution

- Supersedes the previous history-bucket assumption: `codex_model_router_v2` is not a universal fix for history visibility. It only described one old local router bucket. Do not rewrite sqlite/jsonl buckets as the default fix for missing history.
- Do not treat the user's current official/default state as proof that the modified build works. The user had to roll back to official release/default config to keep chatting.
- Confirmed root causes:
  - `CodexAdapter::extract_base_url` previously scanned for the first `base_url` string in TOML, so inactive `[model_providers.*]` and `[mcp_servers.*]` entries could contaminate the active provider.
  - Provider/live merge kept stale provider-owned fields. Official fallback with empty config could retain old `model_provider`, `model_catalog_json`, `experimental_bearer_token`, or old `[model_providers.<router>]`, leaving DeepSeek/Qwen candidates visible after switching backup official.
  - Codex common config could deep-merge provider-private router TOML into arbitrary providers.
  - Proxy takeover official switching needed to exit takeover and restore/write live official config instead of trying to hot-switch through the local proxy.
  - The old `preserve_codex_mcp_servers_from_existing_config` path only preserved MCP servers, not full Codex user sections like `[projects]`, `[features]`, `[desktop]`, `[memories]`.
- Implemented fixes:
  - `src-tauri/src/proxy/providers/codex.rs`: base URL extraction uses `crate::codex_config::extract_codex_base_url`, which prefers the active `model_provider`.
  - `src-tauri/src/services/provider/mod.rs`: Codex credential extraction uses the same active TOML parser; switching an official provider during takeover calls `disable_takeover_for_app_after_switch_lock`, sets current provider, writes official live config, and syncs MCP.
  - `src-tauri/src/codex_config.rs`: official empty config now clears provider-owned top-level fields, removes CC Switch-owned `model_catalog_json`, and removes the active custom `[model_providers.<id>]` table while preserving user sections.
  - `src-tauri/src/services/provider/live.rs`: Codex common config strips `model`, `model_provider`, `model_context_window`, `model_catalog_json`, `experimental_bearer_token`, and `[model_providers]`.
  - `src-tauri/src/services/proxy.rs`: backup/live preservation now uses full Codex provider/live merge rather than MCP-only merge. Added regression test for router takeover -> official fallback cleanup.
- Validation commands passed:
  - `.\node_modules\.bin\tsc.CMD --noEmit`
  - `cargo test codex_switch_to_official_during_takeover_exits_proxy_and_cleans_router_fields --lib`
  - `cargo test test_extract_base_url_uses_active_model_provider_only --lib`
  - `cargo test codex_config --lib` (46 passed)
  - `cargo test codex_common_config --lib` (6 passed)
  - `cargo test provider_switch_with_restored_codex_backup_refreshes_catalog_and_common_config --lib`
  - `cargo test codex_restore_from_backup_projects_inline_model_catalog --lib`
  - `.\node_modules\.bin\tauri.CMD build --no-bundle`
- Latest delivery artifacts:
  - Directory: `src-tauri/target-ccswitchmulti-rootfix-20260609_032709/`
  - `CCSwitchMulti.exe` SHA256 `D764449F06FEEEA7FED052693AB55EE26200C2609B1001DBD56EE993F4186123`
  - `CCSwitchMulti_3.16.1-1_x64-rootfix-portable.zip` SHA256 `46BB69EB96FD811B945152EC2672C6220E0FC545DE47AD6326CE69E8C31C5AB9`
  - `CCSwitchMulti_3.16.1-1_x64-setup.exe` SHA256 `73F7E05581E35278936420CF5F5E13229A383D08F26FB960E689336395B67635`
  - `CCSwitchMulti_3.16.1-1_x64_en-US.msi` SHA256 `9E093D8C493E52337DD1811B8081A8187372C17CF384AC605C7EE4BA0DCFB132`
- Packaging notes:
  - Full `tauri build` produced NSIS/MSI but returned 1 because updater signing has a public key and no `TAURI_SIGNING_PRIVATE_KEY`; use `tauri build --no-bundle` to verify portable exe without signing.
  - Old timestamp package dirs `target-ccswitchmulti-20260609_001033` and `target-ccswitchmulti-historyfix-20260609_013447` were removed after creating the rootfix package. Only the rootfix directory should be handed out now.
  - The current running official app remained `C:\Users\sunda\AppData\Local\Programs\CC Switch\cc-switch.exe`; this rootfix pass did not stop it and did not mutate live `%USERPROFILE%\.cc-switch` or `%USERPROFILE%\.codex` config.

## 2026-06-09 Rootfix DB Provider Write

- After packaging rootfix, the current `%USERPROFILE%\.cc-switch\cc-switch.db` still only had `codex-official` and stale `default`; the package fix alone did not write the user's Codex provider config.
- DB backup before writing: `%USERPROFILE%\.cc-switch\backups\db_backup_before_codex_rootfix_config_20260609_145601.db`.
- Current Codex provider set written to DB:
  - `codex-official` / `OpenAI Official Backup`: official fallback, current provider, empty config/auth.
  - `codex-openai-router` / `OpenAI Multi-Model Router`: local proxy provider with `model_provider="codex_model_router_v2"`, base URL `http://127.0.0.1:15721/v1`, catalog models `gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.3-codex-spark`, `qwen3.6`, `deepseek-v4-flash`, `deepseek-v4-pro`, and `codexRouting` routes `openai-official`, `qwen-local`, `deepseek`.
  - `codex-qwen-local` / `Qwen Local vLLM`: direct optional provider for `qwen3.6`, base URL `https://www.matrixminecraft.cn:24443/vllm/v1`, Chat upstream metadata.
  - `codex-deepseek` / `DeepSeek`: direct optional provider for `deepseek-v4-flash` and `deepseek-v4-pro`, base URL `https://api.deepseek.com`, Chat upstream metadata.
- Removed stale provider `default`; it was an imported old router config under a misleading name.
- Cleaned `common_config_codex` by removing provider-owned lines `model_catalog_json`, `model_context_window`, `model_provider`, and `model`; preserved user MCP/plugin/windows/reasoning/auto-compact settings.
- Left Codex proxy disabled and current provider as `codex-official`: `enabled=0`, `proxy_enabled=0`, `live_takeover_active=0`. This avoids disrupting official fallback until the user explicitly enables/switches router.
- UI caveat: already-open CCSwitchMulti windows cache the provider list. Restart/refresh CCSwitchMulti after this DB write to show the four providers.

## 2026-06-09 Current Good Routing State And History Thread Reaudit

- User has now verified this build's Codex routing and OpenAI official fallback configuration are working. Preserve that as the known-good baseline during future debugging.
- Known-good provider layout:
  - `codex-official` / `OpenAI Official Backup`: pure official fallback, empty provider config, safe current provider.
  - `codex-openai-router` / `OpenAI Multi-Model Router`: local proxy provider using active Codex `model_provider = "codex_model_router_v2"` and catalog entries for GPT, Codex Spark, Qwen, and DeepSeek routes.
  - `codex-qwen-local` and `codex-deepseek`: optional direct providers, not replacements for the official fallback.
- Remaining unresolved bug: Codex history threads still do not display/sync as expected. The user says this is related to provider and bucket, and the previous memory around this may be wrong.
- Do not assume `codex_model_router_v2` is a universal history fix and do not rewrite sqlite/jsonl buckets by default. Re-verify Codex Desktop, CCSwitch, and Codex++ behavior around history indexes, provider buckets, accounts, sources, cwd/project filters, archived state, and pagination before implementing a fix.

## 2026-06-09 OpenAI Bucket Semantics And Responses WebSocket Fallback

- Verified against OpenAI Codex docs and local Codex v0.137.0 source: `openai` is a reserved built-in provider id. `model_providers.openai` does not override the built-in provider; `merge_configured_model_providers` keeps the built-in entry. To point built-in OpenAI at a proxy/router, use user-level top-level `openai_base_url`, not `[model_providers.openai].base_url`.
- Built-in `openai` provider semantics that matter for cc-switch:
  - `requires_openai_auth = true`.
  - `wire_api = responses`.
  - `supports_websockets = true`.
  - Normal turns prefer Responses WebSocket before HTTP Responses.
- Root cause of previous `openai` bucket failures/slowness: cc-switch served HTTP `POST /responses` but did not explicitly handle Codex's WebSocket handshake `GET /responses`. Codex switches immediately to HTTP only when the WS connect returns `426 Upgrade Required`; generic 404/405/network failures can cause retries, delay, or timeout.
- Implemented compatibility fix:
  - `src-tauri/src/proxy/server.rs` maps Codex `/responses`, `/v1/responses`, `/v1/v1/responses`, and `/codex/v1/responses` as `GET -> handle_responses_websocket_fallback` and `POST -> handle_responses`.
  - `src-tauri/src/proxy/handlers.rs` adds `handle_responses_websocket_fallback`, returning 426 with a small JSON error. This is an intentional signal to the official Codex client to disable WS for the session and use HTTP.
  - `src/utils/providerConfigUtils.ts` no longer treats `openai_base_url` as a `wire_api` value. Added a regression unit test.
  - `src-tauri/src/codex_history_migration.rs` now gates old v1 helper wrappers behind `#[cfg(test)]`.
- Current DB provider state checked read-only with secrets redacted:
  - `codex-official` / `OpenAI Official Backup` is current and pure official fallback.
  - `codex-openai-router` uses `model_provider = "openai"`, top-level `openai_base_url`, `model_catalog_json`, no `[model_providers.openai]`, routes `openai-official`, `qwen-local`, `deepseek`, and catalog models `gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.3-codex-spark`, `qwen3.6`, `deepseek-v4-flash`, `deepseek-v4-pro`.
- Validation commands passed:
  - `pnpm test:unit tests/utils/providerConfigUtils.codex.test.ts` (26 tests).
  - `cargo test --manifest-path .\src-tauri\Cargo.toml openai_for_v2 --lib` (2 tests).
  - `cargo test --manifest-path .\src-tauri\Cargo.toml responses_websocket_fallback_returns_upgrade_required --lib` (1 test).
  - Focused Rust regressions for `openai_base_url`, router merge, settings migration preservation, and Codex common-config stripping all passed.
- Latest package:
  - Directory: `src-tauri/target-ccswitchmulti-openaibucket-wsfix-20260609_163308/`.
  - Portable exe: `release/bundle/portable/CCSwitchMulti.exe`, SHA256 `DE348E685A03A522B4A2066FD0CAEA900EDE0B50A0433E959897ED4771DFDCC8`.
  - Portable zip: `release/bundle/portable/CCSwitchMulti_3.16.1-1_x64-openai-bucket-wsfix-portable.zip`, SHA256 `0085BAC5C731763D352757A295CC3CEBFF15BFDBCE32FA7BFD0341D56CCD587A`.
  - NSIS installer: `release/bundle/nsis/CCSwitchMulti_3.16.1-1_x64-setup.exe`, SHA256 `3DDD9F93DEF8020CAE12097CCAAFA89807A41C510C40F61696D92353BE2B58BF`.
- Build cleanup: removed default `src-tauri/target` and old `target-ccswitchmulti-rootfix-20260609_032709`. The old rootfix directory was locked by a stale local modified `CCSwitchMulti.exe`, so that stale local process was stopped before deletion. The official installed CC Switch stayed running at `%LOCALAPPDATA%\Programs\CC Switch\cc-switch.exe`.
- Operational note: only `src-tauri/target-ccswitchmulti-openaibucket-wsfix-20260609_163308/` should be handed out now. Launching/testing the new portable no longer has an older CCSwitchMulti process competing via single-instance; it is not necessary to stop the user's official Codex/official backup chat process.

## 2026-06-11 Third-party Agent API Public Access Check

- External OpenAI-compatible Agent API is intentionally separated from the Codex/Multi Router main proxy: current external listener is `0.0.0.0:15722`; main proxy `15721` is not listening in the checked runtime.
- Local and trusted-network reachability passed:
  - `http://127.0.0.1:15722/health` returned HTTP 200.
  - LAN addresses `192.168.31.206:15722` and `192.168.31.152:15722` returned HTTP 200 from this host.
  - Tailscale address `100.118.73.52:15722` returned HTTP 200 from this host.
- Public Internet reachability failed from this host:
  - Public IP discovery returned inconsistent exits (`185.151.146.146` from ipify and `117.133.83.107` from ipinfo), indicating proxy/multi-exit/NAT behavior.
  - `http://185.151.146.146:15722/health` and `http://117.133.83.107:15722/health` both timed out.
- Interpreted cause: CC Switch is bound correctly and Windows has enabled inbound `cc-switch.exe` allow rules for Private/Public profiles, so the remaining blocker is likely upstream of the app: router port forwarding, carrier-grade NAT, public IP not mapped to this machine, or external firewall/NAT policy.
- Do not treat公网 timeout as an application regression unless LAN/Tailscale/localhost also fail. For real public exposure, configure router/NAT port forwarding to the machine's active LAN IP or use a tunnel/VPN endpoint, and keep `ccsw_` keys private.
- Added `docs/guides/external-openai-api-relay-domain-guide-zh.md` as the operational handoff guide for exposing the External OpenAI-compatible API through a public relay/domain. The preferred topology is public relay Caddy/Nginx -> private Tailscale or SSH tunnel -> Windows CC Switch `15722`; use route/NAT forwarding only when a real inbound public IP exists.

## 2026-06-12 Codex DeepSeek Direct Provider Local Routing Fix

- Root cause for the reported standalone DeepSeek Codex provider failure: the UI's "需要本地路由映射" intent was stored as `meta.apiFormat = "openai_chat"`, but `ProviderService::switch` only hot-switched when takeover was already active. In normal mode it still wrote the DeepSeek provider directly into Codex live config, so Codex called `https://api.deepseek.com/responses` and DeepSeek returned 404.
- This is not a Third-party Agent API issue and not a DeepSeek documentation issue. DeepSeek's official endpoint is Chat Completions style; Codex still speaks Responses to CC Switch, so the local proxy must sit between Codex and DeepSeek.
- Regression source audit:
  - `1c82b8a3 Add Chat Completions routing for Codex providers` introduced `meta.apiFormat = "openai_chat"` and the proxy conversion path, while keeping generated Codex `wire_api = "responses"` so the Codex client can continue using Responses locally.
  - The same change only added a frontend warning in `useProviderActions`; it did not block normal switch or enable takeover.
  - Existing `ProviderService::switch` behavior from the older switch architecture still treated "not currently taken over" as permission to call `switch_normal -> write_live_with_common_config`, which direct-writes provider config to Codex live files.
  - Later local changes `8af568e4` / `24eca85c` made the UI present this as a first-class local routing / multi-route capability, which made the latent mismatch user-visible: users reasonably expected the switch/config to activate routing, but the backend still only routed if takeover was already active.
  - Official upstream is not able to make DeepSeek work by direct `/responses` either; it works only when Codex is already going through CC Switch proxy/takeover. The fix here is making that invariant backend-enforced instead of relying on user sequence or frontend warning.
- Implemented backend defense:
  - `ProviderService::codex_provider_requires_local_proxy` detects Codex providers that require local proxy because they are Chat Completions backends or contain `codexRouting`.
  - `ProviderService::switch` now auto-enables Codex takeover for such providers when takeover is not already active, instead of taking the normal direct live-write path.
  - `ProxyService::takeover_app_and_switch_provider_after_switch_lock` performs the locked transition: start proxy if needed, back up/sync existing live config, switch current provider, write Codex live config to local proxy `/v1`, update backup/current target, and set per-app takeover enabled.
- Regression test added: `switching_codex_chat_provider_auto_enables_local_proxy_takeover` asserts a DeepSeek `openai_chat` provider switch writes `http://127.0.0.1:<port>/v1` plus `PROXY_MANAGED` into Codex live config and does not leave `https://api.deepseek.com` in live config.
- Validation passed:
  - `cargo test switching_codex_chat_provider_auto_enables_local_proxy_takeover --manifest-path src-tauri/Cargo.toml --lib`
  - `cargo test test_codex_provider_uses_chat_completions --manifest-path src-tauri/Cargo.toml --lib`
  - `cargo test v1_responses --manifest-path src-tauri/Cargo.toml --lib`
  - `cargo test external_openai_api --manifest-path src-tauri/Cargo.toml --lib`
  - `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
  - `pnpm typecheck`

## 2026-06-12 Codex Takeover Model Picker Must Stay On Built-in OpenAI

- Follow-up root cause for the user's "GPT menu shows 自定义, where did the selectable models go" screenshot: after the DeepSeek auto-takeover fix, Codex live config correctly pointed at CC Switch, but it still projected the selected upstream provider id (`deepseek`, `aihubmix`, etc.) into live `model_provider`. Codex then treated the session as a custom provider and the model picker collapsed into the custom-model bucket instead of showing the intended GPT/router catalog choices.
- Correct invariant: during proxy takeover, Codex live `config.toml` should expose the stable built-in OpenAI provider:
  - `model_provider = "openai"`
  - top-level `openai_base_url = "http://127.0.0.1:<port>/v1"`
  - `model_catalog_json = "cc-switch-model-catalog.json"` when CC Switch has a model catalog
  - `auth.json` uses `OPENAI_API_KEY = "PROXY_MANAGED"`
  - no upstream `[model_providers.<deepseek/qwen/...>]` table should be exposed in live takeover config.
- Real upstream provider identity and API keys stay in CC Switch DB/backup/provider settings. The proxy resolves the current provider or `codexRouting` by request model and injects upstream credentials internally.
- Implemented fix:
  - `ProxyService::apply_codex_proxy_toml_config_for_provider` now projects takeover TOML to built-in `openai` plus `openai_base_url`, preserving the selected model but stripping upstream provider tables/tokens from live config.
  - `codex_config::merge_codex_provider_config_texts` now removes the active custom provider table when the provider projection targets built-in `openai`, so stale live `[model_providers.*]` tables do not survive the merge.
- Regression coverage:
  - `apply_codex_proxy_toml_config_uses_builtin_openai_proxy_provider`
  - `hot_switch_codex_chat_provider_updates_live_provider_display`
  - `merge_openai_router_config_uses_builtin_openai_history_bucket`
  - `switching_codex_chat_provider_auto_enables_local_proxy_takeover`

## 2026-06-12 CCSwitchMulti v3.16.2-2 Release Export Rule

- Release tag for this fix train is `v3.16.2-2`; do not reuse `v3.16.2-1` because it already exists on `BigStrongSun/cc-switch`.
- Fixed local export directory remains `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`.
- GitHub Release assets cannot safely upload two different files both named `BUILD_ON_PLATFORM.md`; the export script now also writes root-level `linux-build-note.md` and `macos-build-note.md` with unique names for release upload.
- `SHA256SUMS.txt` should be generated after those root-level note files are copied, so the checksum list matches the final export directory.

## 2026-06-12 Codex DeepSeek Routing Crash And Legacy Wire API Fix

- User-reported crash: CCSwitchMulti v3.16.2-2 flashed/crashed when enabling Codex routing or switching to the DeepSeek provider.
- Windows/WER plus `%USERPROFILE%\.cc-switch\crash.log` showed the real root cause: `there is no reactor running, must be called from the context of a Tokio 1.x runtime`, followed by `panic in a function that cannot unwind`. This happened because the synchronous Tauri `switch_provider` command called `futures::executor::block_on` and then started the proxy TCP listener outside a Tokio reactor.
- Fix invariant: synchronous provider commands that wait for async proxy/db work must use a Tauri-runtime-aware helper. If a Tokio handle is already present, continue polling in the current context; otherwise enter `tauri::async_runtime::block_on`.
- Implemented helper: `services::provider::block_on_tauri_runtime`, used by provider switch/update/sync paths that call proxy async methods.
- Regression test added: `switching_codex_chat_provider_from_sync_command_has_tokio_reactor`, which simulates the desktop synchronous command path and verifies switching a Codex Chat provider starts local proxy without the missing-reactor panic.
- Second root cause found in current user DB (read-only, secrets redacted): `codex-deepseek` had `base_url = "https://api.deepseek.com"` and model catalog entries, but `wire_api = "responses"` and no `meta.api_format`. The old detector returned false as soon as it saw `wire_api = "responses"`, so DeepSeek was treated like a Responses provider and Codex could call `/responses` directly.
- Fix invariant: explicit `meta.api_format` still wins, but known Chat-Completions-only upstream URLs such as `api.deepseek.com`, `api.moonshot.cn`, DashScope, GLM, SiliconFlow, OpenRouter, and vLLM must be detected before trusting stale `wire_api = "responses"` from historical configs.
- Regression tests added:
  - `test_codex_provider_uses_chat_completions_for_legacy_deepseek_responses_wire_api`
  - `test_codex_provider_keeps_openai_responses_wire_api`
- This bug is not caused by the Third-party Agent API. It is the Codex provider/takeover path plus stale provider wire metadata.

## 2026-06-12 Codex Router Official GPT-5.5 URL Normalization Fix

- User clarified that the failed high-demand/reconnect case happened after selecting `gpt-5.5` from the Codex model list, while `OpenAI Official Backup` could use `gpt-5.5` successfully.
- Root cause: the Codex multi-model router's managed OAuth route builds a temporary `codex_oauth` provider that uses `CodexAdapter`. `CodexAdapter.build_url` treated `https://chatgpt.com/backend-api/codex` like a generic custom prefix, so a local Codex request to `/v1/responses` could become `https://chatgpt.com/backend-api/codex/v1/responses`. ChatGPT's Codex backend expects `https://chatgpt.com/backend-api/codex/responses` without `/v1`.
- Why official backup worked: non-router official requests were already observed in `codex-router.log` as `upstream_url=https://chatgpt.com/backend-api/codex/responses`. The bug lived in the router/effective-provider URL construction path, not in the user's official subscription, model availability, or DeepSeek conversion.
- Fix invariant: any Codex OAuth provider targeting `https://chatgpt.com/backend-api/codex` must strip the OpenAI-compatible `/v1/` prefix before forwarding to ChatGPT Codex backend. `/v1/responses` maps to `/responses`; `/v1/responses/compact?...` maps to `/responses/compact?...`.
- Regression tests added/strengthened:
  - `test_build_url_chatgpt_codex_backend_strips_openai_v1_prefix`
  - `test_codex_adapter_supports_routed_codex_oauth_provider` now asserts routed OAuth URL construction as well as auth strategy.

## 2026-06-12 Codex Multi Router 首个 SSE 错误触发 Failover

- 用户继续反馈 CCSwitchMulti 的 Codex multi 选择多路路由后仍出现 `We're currently experiencing high demand` / `stream disconnected before completion`；恢复 `OpenAI Official Backup` 也可能报同类错误。
- 追根因后确认：这类错误不一定表现为 HTTP 5xx。ChatGPT/Codex OAuth 可能返回 HTTP 200 + `text/event-stream`，但首个 SSE block 就是 `event: error` 或 `event: response.failed`。此前 `RequestForwarder::prime_streaming_response` 只等到首个 chunk 就把 provider 记为成功并把响应交给 Codex；一旦响应头已发给客户端，同一个请求就不能再 failover 到下一路。
- 修复规则：在首包预读阶段解析首个完整 SSE block；如果明确是 `error` / `response.failed` / payload 中含 `error` 或 `response.status=failed`，在响应交给客户端前转换为 `ProxyError::UpstreamError { status: 503 }`。这样现有 retry/failover 分类会把它当作可重试上游失败，multi 路由/故障转移才有机会换下一家。
- 正常 `response.created`、delta、`response.completed` 仍必须原样 replay 给客户端，不能为了吞错而破坏正常流。
- 已加回归测试：
  - `streaming_first_sse_error_event_is_retryable_before_response_is_returned`
  - `streaming_first_normal_sse_event_is_replayed_to_client`
- 已验证：
  - `cargo test streaming_first --manifest-path src-tauri/Cargo.toml --lib`
  - `cargo test forwarder --manifest-path src-tauri/Cargo.toml --lib`
  - `cargo test test_build_url_chatgpt_codex_backend_strips_openai_v1_prefix --manifest-path src-tauri/Cargo.toml --lib`
  - `cargo test test_codex_adapter_supports_routed_codex_oauth_provider --manifest-path src-tauri/Cargo.toml --lib`
  - `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
  - `cargo check --manifest-path src-tauri/Cargo.toml`（仅既有 `commands/misc.rs` 两个 unused warning）

## 2026-06-12 Codex Official 也报 high demand 的根因修正

- 用户指出“official 也出现 high demand，说明上游返回 error 本身就不对，前一刀没修到点上”。这个判断成立：上一条 `prime_streaming_response` 修复只解决“首个 SSE error 交给客户端前还能 failover”的边界，不解释为什么 official/official backup 会拿到同类错误。
- 本机排查结论：恢复到 official backup 后，`~/.codex/config.toml` 已没有 `model_provider/openai_base_url/cc-switch` takeover 字段，主代理也已停止；纯 official 路径不经过 CC Switch。此时仍出现 high demand，只能是官方 Codex/ChatGPT 后端或 official 客户端重试后仍失败，CC Switch 不能在纯直连 official 路径里修上游容量错误。
- 对比 `codex-source-rust-v0.137.0` official 源码后确认：official Codex 会使用 `session-id`、`thread-id`、`x-client-request-id`、`x-codex-window-id = {thread_id}:{generation}`，并通过 `responses_retry::handle_retryable_response_stream_error` 对可重试 stream 错误循环重试，必要时 WebSocket fallback 到 HTTPS。
- CC Switch 的 official/managed OAuth 代理路径此前不够 official：`extract_codex_session` 只认 `session_id/x-session-id` 并给值加 `codex_` 前缀；`build_codex_oauth_session_headers` 注入 `session_id` 下划线头，且会覆盖已有 header。这会让“OpenAI Official Backup / router official route”在代理路径中和 official 客户端的身份/缓存/路由信号不一致，可能放大 high-demand/stream-failed 问题。
- 根因修复：Codex session 提取现在识别 `session-id/thread-id/x-client-request-id/x-codex-window-id/session_id/x-session-id`，从 `x-codex-window-id` 提取 thread_id，并保留原始值不加前缀；ChatGPT Codex OAuth 转发补齐 `session-id/thread-id/x-client-request-id/x-codex-window-id`，且只在原请求缺失时补，不覆盖 official 客户端已有值。
- 回归测试新增/更新：
  - `test_codex_official_session_id_header_is_preserved`
  - `test_codex_window_id_header_extracts_thread_identity`
  - `codex_oauth_session_headers_match_codex_cache_identity`
- 已验证：
  - `cargo test codex --manifest-path src-tauri/Cargo.toml --lib`（357 tests）
  - `cargo test forwarder --manifest-path src-tauri/Cargo.toml --lib`（52 tests）
  - `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
  - `cargo check --manifest-path src-tauri/Cargo.toml`（仅既有 `commands/misc.rs` 两个 unused warning）

## 2026-06-12 Codex Multi Router 从“模型分流”升级为“路由内故障转移”

- 用户继续指出“选择多路路由仍报 high demand，说明上游返回 error 本身就不对，之前没修到点上”。再次追根因后确认：当前 `codex-openai-router` 配置里，`gpt-5.5` 只匹配 `openai-official` route；Qwen/DeepSeek route 只匹配各自模型名前缀。旧逻辑的“多路路由”只是按请求模型选一路，不是同一个请求在官方失败后自动尝试其它 route。
- 因此即使首个 SSE `event:error` 已能在响应交给客户端前变成 retryable error，外层 failover 也只有一个 router provider 可尝试；实际不会落到 Qwen/DeepSeek。要真正解决“官方高负载时多路路由继续跑”，必须把 router provider 在转发前展开成 route provider 候选链。
- 修复规则：Codex 请求进入 `RequestForwarder::forward_with_retry_inner` 后，如果当前 provider 是 Codex router，就按请求模型解析候选 route：匹配 route 放第一位；其它 enabled route 作为后备追加。外层 provider retry/failover 会逐个尝试这些 effective provider。
- 跨模型后备必须改写上游模型名：例如用户请求 `gpt-5.5` 时，第一路 official 仍发 `gpt-5.5`；若 official 首包失败并切到 DeepSeek route，发给 DeepSeek 的模型必须改成 route 自己的默认模型（如 `deepseek-v4-flash`），不能把 `gpt-5.5` 原样发给 DeepSeek/Qwen。
- 为避免展开后的 route provider 再次被解析回官方 route，resolved route 会带 `codexResolvedRouteId`；`forward` 看到该标记后直接使用该 effective provider。
- 回归测试新增：
  - `test_codex_router_returns_fallback_route_candidates_after_primary`
  - `test_apply_codex_chat_upstream_model_forces_unmatched_fallback_route_model`
- 已验证：
  - `cargo test test_apply_codex_chat_upstream_model_forces_unmatched_fallback_route_model --manifest-path src-tauri/Cargo.toml --lib`
  - `cargo test codex_router_returns_fallback_route_candidates_after_primary --manifest-path src-tauri/Cargo.toml --lib`
  - `cargo test forwarder --manifest-path src-tauri/Cargo.toml --lib`（52 tests）
  - `cargo test codex --manifest-path src-tauri/Cargo.toml --lib`（359 tests）
  - `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
  - `cargo check --manifest-path src-tauri/Cargo.toml`（仅既有 `commands/misc.rs` 两个 unused warning）

## 2026-06-12 Codex Multi Router official route 与 official backup 不等价

- 用户继续追问“为什么 Multi Router 用 official 会失败，这才是本质”。排查结论：Multi Router 的 official route 不是纯 official backup；它是 Codex built-in `openai` bucket -> `openai_base_url=http://127.0.0.1:<port>/v1` -> CC Switch HTTP/SSE proxy -> `https://chatgpt.com/backend-api/codex/responses`。
- 官方 Codex 源码 `model-provider-info/src/lib.rs::create_openai_provider` 对 built-in `openai` 设置 `supports_websockets = true`；`client.rs` 会优先走 Responses WebSocket，失败后才通过 `responses_retry::handle_retryable_response_stream_error` fallback 到 HTTPS/SSE。CC Switch 当前主代理没有实现 Codex Responses WebSocket，只在 `/responses` 的 GET 上返回 426 让客户端降级。
- 因此“Multi Router official”比“official backup”少了官方 WebSocket 直连能力，更容易落到 GitHub issue 中大量用户也报错的 HTTPS/SSE `/backend-api/codex/responses` 路径。外部 issue 覆盖 `stream disconnected before completion`、`high demand`、remote compaction、Azure/rate-limit/context 等场景；这说明 high demand 文案是 Codex 对多类后端/传输失败的泛化提示，不一定只表示真实排队高峰。
- 之前保留 `model_provider="openai"` 是为了维持官方 history bucket 和模型菜单；但这个选择天然启用 built-in OpenAI WebSocket 语义。若要让 Multi Router official 真正等价 official backup，根修方向不是再补 HTTP retry，而是实现 Codex Responses WebSocket relay/proxy，至少覆盖 prewarm、response.create、`x-codex-turn-state` sticky routing、`response.processed` 等官方协议。
- 可选降级方案：改回自定义 provider 并显式 `supports_websockets=false` 可避免 WS fallback 抖动，但会重新带来模型菜单/历史 bucket 变成自定义的问题；这是产品取舍，不是根治。

## 2026-06-12 Codex Responses WebSocket official relay

- 用户强调“尽量复用官方，不然永远会有 bug”。本轮修复原则：CC Switch 不实现自己的 Codex 事件协议解释器，只在本地 `/responses` GET 接受 WebSocket 后做透明中继；官方事件流、`response.create`、`response.processed`、prewarm 完成事件、错误事件都由 Codex 官方客户端和 ChatGPT Codex 后端继续按原协议处理。
- 新增 `src-tauri/src/proxy/codex_ws.rs`：首帧只解析 `response.create` 的 JSON 以获取 `model`，复用现有 `resolve_codex_model_routed_providers` 和 `CodexAdapter` 判定真实 route；只有 route 上游是 `https://chatgpt.com/backend-api/codex` 且不是 Chat Completions-only 时，才连接 `wss://chatgpt.com/backend-api/codex/responses`。
- WebSocket upstream 鉴权复用现有 Codex OAuth 托管账号：从 `CodexOAuthState` / `CodexOAuthManager` 取真实 access token，再通过 `CodexAdapter::get_auth_headers` 生成 `authorization` / `originator`；同时透传 official 相关 header：`session-id`、`thread-id`、`x-client-request-id`、`x-codex-window-id`、`x-codex-turn-state`、`chatgpt-account-id` 等。
- 非 official WS 路线不能在升级后直接断流，否则 official Codex 会报 `stream disconnected before completion`。正确做法是发送官方源码 `responses_websocket.rs` 能解析的 `{"type":"error","status_code":426,...}`，让 `client.rs` 命中 `WebsocketStreamOutcome::FallbackToHttp`，再走现有 HTTP Responses -> Chat bridge 给 Qwen/DeepSeek 等第三方 API。
- 路由变更：`/responses`、`/v1/responses`、`/v1/v1/responses`、`/codex/v1/responses` 的 GET 进入 `handle_responses_websocket`；非升级 GET 仍返回旧 426 JSON，POST HTTP Responses 路径不变。External OpenAI API 独立端口的 `/v1/responses` GET 也复用同一官方 fallback/relay handler，POST 仍走 external profile。
- 新增依赖：`axum` 开启 `ws` feature，新增 `tokio-tungstenite` 的 rustls/webpki TLS feature。
- 已验证：
  - `cargo test proxy::codex_ws`
  - `cargo test proxy::providers::codex`
  - `cargo test proxy::server`
  - `cargo fmt --check`
  - `cargo check`（仅既有 `commands/misc.rs` 两个 unused warning）

## 2026-06-12 Codex WS close normally after Multi Router

- 用户反馈新 WS relay 后 Multi Router 报 `stream disconnected before completion: failed to send websocket request: Connection closed normally`。这说明本地 `/responses` WS 已被 official Codex 命中，且到 ChatGPT Codex upstream 的 WebSocket 握手成功，但上游在首个 `response.create` 发送前/发送时正常关闭。
- 对照官方源码确认：`core/src/client.rs::build_websocket_headers` 会构造 `openai-beta: responses-websockets-v2`、`x-codex-beta-features`、`x-codex-turn-state`、`x-codex-turn-metadata`、`x-client-request-id`、`session-id`、`thread-id`、`x-codex-window-id`、attestation 等；随后 `codex_login::default_client::default_headers()` 补 `originator` 和真实 `user-agent`。上一版 relay 只手写少数头，并通过 `CodexAdapter::get_auth_headers` 把 `originator: cc-switch` 发给 upstream WS，不够 official。
- 修复规则：上游 WS 握手应优先复用客户端发给本地代理的官方 headers；只过滤 hop-by-hop/WebSocket 握手头、本地占位 `authorization`、content headers，然后替换为真实 Codex OAuth `Authorization`。不要覆盖客户端提供的 `originator`、`user-agent`、`openai-beta`、`x-codex-*`、attestation 等官方头。
- 代码位置：`src-tauri/src/proxy/codex_ws.rs::copy_official_client_headers` 与 `should_skip_client_ws_header`。`codex_auth_headers` 仍负责取托管 OAuth token，但插入 upstream headers 时跳过 adapter 生成的 `originator`，避免把官方 originator 改成 `cc-switch`。
- 已验证：
  - `cargo fmt --check`
  - `cargo test proxy::codex_ws`
  - `cargo check`
  - `pnpm typecheck`
  - `pnpm release:export`
- 新 raw exe 已导出并启动：`C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti\windows\raw-exe\CCSwitchMulti.exe`，SHA256 `6A14F9627A87DBFA274D28D8A45703B7B05511145DA431D30F4B1E15770D3D11`。

## 2026-06-12 Codex WS Connection closed normally diagnostics

- 用户继续反馈开启 Multi Router 后仍报：`stream disconnected before completion: failed to send websocket request: Connection closed normally`。本轮先查日志：`%USERPROFILE%\.cc-switch\logs\cc-switch.log` 只有代理启停，`codex-router.log` 只有旧 HTTP forwarder 事件，缺少 Responses WebSocket relay 的握手、首帧、close code、fallback event 发送结果，因此无法判断是本地代理提前关、官方 upstream policy close，还是 fallback event 没送到 Codex 客户端。
- 外部交叉验证：Codex built-in web search 与用户 `matrix-websearch` 均搜到 openai/codex 同类问题；典型 issue 包括 `openai/codex#13039` / `#13041`，证据是 `wss://chatgpt.com/backend-api/codex/responses` 握手 `101 Upgrade` 成功后，官方 upstream 立即发 close code `1008 Policy`，Codex 客户端显示同样的 `failed to send websocket request: Connection closed normally` 并 fallback 到 HTTPS。因此本地日志必须记录 close code/reason length 和是否收到上游首帧，不能只记录 relay done。
- 诊断增强：`src-tauri/src/proxy/codex_ws.rs` 新增 `ws_*` 事件写入 `codex-router.log`，包含 accepted/client*first_frame/route_resolved/upstream_connect_start/upstream_connect_ok/upstream_first_send_start/upstream_first_send_ok/upstream_first_frame/upstream_close/client_close/relay*\*\_done/error/fallback_event_send_ok/error/fallback_close_ok/error 等。日志只写 header 名、帧类型、字节数、close code、reason_len 和 JSON error 摘要，不记录 token、header value、完整首帧、完整 upstream text、完整 close reason。
- 行为修正：若 upstream 首帧发送失败，不能直接 close 本地 WS；现在会先记录 `ws_upstream_first_send_error` 和 500ms upstream probe，再向本地 Codex 发送协议内 `status_code=426` error event，触发官方客户端按自身逻辑 fallback 到 HTTP Responses，而不是让用户只看到 `Connection closed normally`。
- Relay 可观测性增强：`upstream_first_send_ok` 之后的透明转发阶段会统计两侧 frames/bytes；如果 upstream 正常 close，会记录 `ws_upstream_close code=<code> reason_len=<n> before_first_upstream_frame=<bool>`；如果没有任何 upstream frame 就结束，会记录 `ws_upstream_ended_without_frames`。这正是后续区分“官方上游 policy close 1008”和“本地 relay/fallback 未送达”的关键证据。
- 本轮验证：
  - `cargo fmt --check`
  - `cargo test proxy::codex_ws`
  - `cargo check`（仅既有 `commands/misc.rs` 两个 unused warning）
  - `pnpm typecheck`
  - `pnpm release:export`
- 新 raw exe 已导出并启动：`C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti\windows\raw-exe\CCSwitchMulti.exe`，SHA256 `4AC80A8E65784438957618568F7C1547B56BBD9381EF9B8FC7849CD87F4EDE1C`。启动后 `http://127.0.0.1:15722/health` 正常；`15721` 在未启用 Codex takeover 时不监听，符合预期。

## 2026-06-12 Codex Multi Router not being hit runtime check

- 用户再次反馈同样 `Connection closed normally`，但检查结果显示这次请求没有进入 CC Switch 的 Codex Multi Router：`%USERPROFILE%\.cc-switch\logs\codex-router.log` 最后更新时间仍是 `2026-06-12 06:16:39 UTC`，没有任何新 `event=ws_*`；`~/.codex/config.toml` 当前没有 `model_provider` / `openai_base_url` 指向 `127.0.0.1:15721`；`http://127.0.0.1:15721/health` 不通，而 `15722/health` 正常。
- `cc-switch.log` 显示用户在 `2026-06-12 16:45:20` 选择 `codex-openai-router` 后确实短暂启动了 Codex takeover 并写入 `http://127.0.0.1:15721/v1`，但 `16:46:17` 又执行了 Codex Live 配置恢复并停止 15721。用户说明这是因为不可用后切回 official，因此后续报错自然不会有 router 日志。
- 当前数据库状态：`providers` 里 `codex-official` 是 `is_current=1`，`codex-openai-router` 是 `is_current=0`；`proxy_config` 里 `codex.enabled=0`；`proxy_live_backup` 为空；第三方 OpenAI API 旁路 profile 仍指向 `codex-official`。因此现状是纯 official/旁路 official，不是 Multi Router takeover。
- 重要使用判据：Codex Multi Router 给 Codex 客户端用的是 `15721` takeover 端口；`15722` 是第三方 OpenAI-compatible Agent API 旁路端口，两者不是同一路。要验证 Multi Router，必须先在 CCSwitchMulti 选择 `OpenAI Multi-Model Router`，确认 `15721/health` 正常且 `~/.codex/config.toml` 指向 `127.0.0.1:15721/v1`，然后新开/重启 Codex 会话，因为已经运行的 Codex 会话通常不会重新读取刚改的 config。

## 2026-06-12 Codex Desktop App Multi Router activation diagnostics

- User clarified that "Codex" in this issue means the OpenAI Codex Desktop App, not a standalone CLI. The user's manual switch back to official/route-off was only to keep the current Codex conversation usable for debugging and must not be treated as the root cause.
- Local process evidence: the Desktop App runs `Codex.exe` from the WindowsApps package and an agent process `resources\codex.exe app-server --analytics-default-enabled`. In the current manual-official state, CCSwitch listens on `15722` only and `15721` is not listening, which is expected.
- Official documentation context: user-level `~/.codex/config.toml` supports `openai_base_url` as the built-in `openai` provider base URL override. The documentation warning that Codex ignores `openai_base_url` applies to project-local `.codex/config.toml`, not the user-level file.
- Code change: `ProxyService::takeover_app_and_switch_provider_after_switch_lock` now verifies the final activation state after starting the proxy, writing live config, setting DB enabled, and setting active target.
- New log event: `takeover_activation_check app=... provider=... proxy_running=... expected_proxy_url=... expected_codex_base_url=... live_matches_current_proxy=...`. Failure logs `takeover_activation_failed ... config_path=...` and rolls back provider/enabled/live config so the UI cannot show a false successful Multi Router activation.
- Next diagnostic rule: if Multi Router switch logs `proxy_running=true` and `live_matches_current_proxy=true` but `codex-router.log` still has no request, the remaining root cause is Codex Desktop app-server/thread not refreshing user config; if the activation check fails, follow the logged port/config evidence first.

## 2026-06-12 Codex Multi Router WS route/fallback root cause

- 完整追溯后确认链路：UI provider 卡片 -> `useProviderActions.switchProvider` -> `useSwitchProviderMutation` -> Tauri `switch_provider` -> `ProviderService::switch`。Codex router provider 因 `settings_config.codexRouting` 被判定为必须走本地代理，后端调用 `takeover_app_and_switch_provider_after_switch_lock`，启动 15721、备份 live config、写入 `openai_base_url=http://127.0.0.1:15721/v1`，并把当前 provider 设为 `codex-openai-router`。
- 能关闭 15721 的源码路径只有：切换到 category=official 的 provider 时走 `disable_takeover_for_app_after_switch_lock`；顶部/设置页关闭 takeover 时走 `set_takeover_for_app(false)`；总关闭代理时走 `stop_with_restore`。列表查询/provider 查询/get status 不会自动关闭 15721。
- 当前运行态证据：`15721/health` 不通，`15722/health` 正常；DB 中 `codex-official is_current=1`、`codex-openai-router is_current=0`、`proxy_config.codex.enabled=0`；`codex-router.log` 最后更新时间仍是 `2026-06-12 06:16:39 UTC`，因此这次用户看到的后续报错没有进入 15721。
- 中转根因修复：`codex_ws::resolve_official_ws_provider` 以前会遍历 router 展开的所有 fallback route，导致非 official/chat route 命中后仍可能扫描到后面的 official route 并错误进入 official WebSocket。现在只看本次模型解析出的第一条 effective route：如果它是 Chat Completions route 或不是 ChatGPT Codex official upstream，立即发送协议内 426 fallback，让官方 Codex 走 HTTP Responses -> Chat bridge。
- 中转根因修复：official upstream WS 在首帧后立即 close 或无任何数据结束时，旧 relay 只是把 close 原样转给 Codex，客户端显示 `Connection closed normally`。现在在 `upstream_close` 且 `before_first_upstream_frame=true` 或 `upstream_ended_without_frames` 时，向本地 Codex 发送 WebSocket 内 `status_code=426` error event 并关闭，尽量触发官方 HTTP fallback/failover。
- 中转兼容修复：upstream WS `origin` 现在强制覆盖为 `https://chatgpt.com`，避免客户端经本地代理留下非官方 origin 后被 upstream policy close。
- 可观测性修复：official switch 和手动关闭 takeover 现在都会在主日志显式记录 `source=official_switch` 或 `source=proxy_toggle_or_command`，后续能直接看出是谁关闭了 15721。
- UX 修复：Codex provider 切换成功后会刷新 `proxyStatus/proxyRunning/proxyTakeoverStatus/liveTakeoverActive`；即使之前弹过“需要代理”警告，Codex Multi Router 仍会明确提示“保持 CC Switch 运行，并完全重启或新开 Codex 会话后生效”。
- 联网交叉验证：内置 web search 与 matrix-websearch 都能找到 Codex `stream disconnected before completion` 同类问题；matrix 结果更偏中文代理/证书/长连接排障，GitHub 精确结果少。结论是 official 上游/网络确实可能断，但 CC Switch Multi Router 的责任是把可 fallback 的 WS 失败转成 HTTP/failover 路径。
- 已验证：`cargo fmt`、`cargo test proxy::codex_ws --lib`（5 tests）、`cargo check`（仅既有 `commands/misc.rs` 两个 unused warning）、`pnpm typecheck`、`pnpm release:export`。已启动新 raw exe：`C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti\windows\raw-exe\CCSwitchMulti.exe`，SHA256 `BEC4C9F4B41736D26E0238EC5E77A79A9E1A5E3624280884FF42967D5C009C50`。启动后 `15722/health` 正常，未启用 Codex takeover 时 `15721` 不监听，符合预期。

## 2026-06-13 Codex MultiRouter custom runtime boundary

- 覆盖旧结论：MultiRouter 的 Codex live runtime 不能改回 `model_provider="openai"`。`openai` 是 Codex 内置保留 provider，会重新启用官方 OpenAI/WebSocket 语义；之前用它保历史桶和官方模型菜单的方案会把 `Connection closed normally` / WebSocket fallback 老问题带回来。
- 当前正确边界：MultiRouter takeover 写入 `model_provider="custom"`，`[model_providers.custom].base_url=http://127.0.0.1:<codex-port>/v1`，`wire_api="responses"`，`supports_websockets=false`，并移除 `openai_base_url`。真实 OpenAI/Qwen/DeepSeek 上游、API 格式和转换层都留在 `codexRouting` 与后端 route resolver 内处理。
- 模型菜单问题不要通过改回 `openai` 解决；应检查 `modelCatalog` 是否从 DB 投影到 `~/.codex/cc-switch-model-catalog.json`，以及 live config 顶层 `model_catalog_json="cc-switch-model-catalog.json"` 是否存在。Codex 官方只读取顶层 `model_catalog_json`，不是 `[model_providers.*]` 内字段。
- 历史记录问题本质是 Codex Desktop 按 `model_provider` provider bucket 过滤。使用 custom runtime 后，openai 历史不会天然显示在 custom 桶里；修复必须是用户显式触发的历史桶同步/迁移，不能为了历史把 runtime provider 改回 openai。
- MultiRouter 状态页流量统计不能只按真实 `targetProviderId` 聚合。Qwen/DeepSeek 等内联 route 可能没有外部 providerId，应按 route id/label 作为“子 Provider”统计，并可从 `codex-router.log` 的 `route_id` 或 `effective_provider=...::route::<id>` 回归属。

## 2026-06-13 Codex MultiRouter custom provider 候选模型显示修复

- 旧版能显示全量候选的真实路径不是单纯 `/v1/models`，而是 `model_provider="openai"` + `openai_base_url=http://127.0.0.1:<port>/v1` + 顶层 `model_catalog_json="cc-switch-model-catalog.json"`。因为它仍然伪装成 Codex built-in OpenAI provider，所以运行中模型管理器允许刷新 `/models`，能从 CC Switch 本地代理拿到完整 catalog。
- 当前 MultiRouter 不能改回 `openai`，否则会重新进入 built-in OpenAI/WebSocket 语义，带回 `Connection closed normally` / WebSocket fallback 老问题。正确 runtime 仍是 `model_provider="custom"`、`supports_websockets=false`、`base_url=127.0.0.1:<codex-port>/v1`。
- 对照 Codex official 源码确认：如果 Codex 进程启动时读到了顶层 `model_catalog_json`，会走 `StaticModelsManager`，完整 catalog 可直接显示；但如果是在运行中的 Codex 热切到 custom provider，旧的 OpenAI-compatible manager 不会主动刷新 `/models`，`OnlineIfUncached` 只会读 fresh `~/.codex/models_cache.json`。因此只写 `cc-switch-model-catalog.json` 不足以修复热切后的候选模型列表。
- 根因修复：CC Switch 在生成 `~/.codex/cc-switch-model-catalog.json` 后，同步写入 `~/.codex/models_cache.json`，复用现有 `client_version`，并用 `etag="cc-switch-model-catalog"` 标记所有权；退出 MultiRouter/切回 official 时，如果当前 cache 是 CC Switch 接管过的，就恢复 `models_cache.cc-switch-backup.json`，避免污染 official backup。
- 这次修复覆盖 Qwen/DeepSeek 候选缺失和 OpenAI GPT speed tier 不显示的同源问题：catalog 生成测试确认 speed tier 没丢，cache 同步测试确认 custom provider picker 能看到 `qwen3.6` / `deepseek-v4-flash`。如果之后还有候选缺失，优先检查 `models_cache.json` 的 `client_version` 是否和当前 Codex app-server 匹配，以及 Codex 是否仍拿旧进程内 catalog。

## 2026-06-13 Codex MultiRouter provider bucket correction

- Updated conclusion after comparing older 2026-06-09 backups: MultiRouter must not use the built-in `openai` provider, but it also should not be flattened into the generic `custom` provider. The old working shape used `model_provider="codex_model_router_v2"` plus `[model_providers.codex_model_router_v2].base_url=http://127.0.0.1:<codex-port>/v1`, top-level `model_catalog_json="cc-switch-model-catalog.json"`, `wire_api="responses"`, and `supports_websockets=false`.
- Root cause for the "only three OpenAI models" symptom: after the 2026-06-12 custom-runtime change, MultiRouter takeover wrote `model_provider="custom"`. That avoided built-in OpenAI WebSocket behavior but lost the router-specific provider bucket used by the old model/history path. Cache sync alone was too weak as a hot-switch repair if Codex kept using the official/openai picker state.
- Code rule: normal single upstream Codex providers still use `CC_SWITCH_CODEX_MODEL_PROVIDER_ID = "custom"`; only providers with enabled `settings_config.codexRouting.routes` use `CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID = "codex_model_router_v2"`. Do not fix this by reintroducing top-level `openai_base_url`; official Codex only applies `openai_base_url` to the built-in `openai` provider, which re-enables the old WebSocket semantics.
- Regression coverage added: router switch now asserts live config uses `codex_model_router_v2`, defines `[model_providers.codex_model_router_v2]`, removes `openai_base_url`, disables websockets, writes `cc-switch-model-catalog.json`, and replaces `models_cache.json` with seven slugs (`gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.3-codex-spark`, `qwen3.6`, `deepseek-v4-flash`, `deepseek-v4-pro`) while preserving `client_version`.

## 2026-06-14 Codex Desktop three-model picker runtime boundary

- Current live config/catalog evidence can be healthy while the visible Desktop picker remains stale. On this machine, `~/.codex/config.toml` pointed at `model_provider="cc_switch_codex_router"` with `model_catalog_json="cc-switch-model-catalog.json"`, local `base_url=http://127.0.0.1:15721/v1`, `wire_api="responses"`, `requires_openai_auth=false`, `supports_websockets=false`, and no `openai_base_url`; `cc-switch-model-catalog.json` contained seven models.
- Fresh `codex.exe debug models` reading the same disk config returned all seven slugs, proving the written TOML/catalog were parseable. Therefore the remaining "only three models" symptom is not explained by route config, DB modelCatalog generation, or 15721 reachability alone.
- Codex Desktop uses `codex.exe app-server --analytics-default-enabled`; app-server builds `ThreadManager.models_manager` from startup config. `model/list` goes through that in-memory manager, so a running app-server can keep an older three-model picker even after CCSwitch rewrites `config.toml` or `cc-switch-model-catalog.json`.
- Concrete runtime evidence from this machine: `cc-switch-model-catalog.json` had 7 models; catalog mtime was `2026-06-13T23:43:49+08:00`; Codex app-server started at `2026-06-13T23:44:11+08:00`; config was written again at `2026-06-13T23:44:34+08:00`. That ordering means Desktop may be holding a model manager created before the final live config write.
- New diagnostic rule: MultiRouter status must show Codex Desktop/app-server process count, app-server command line/start time, config mtime, catalog mtime, catalog model count, and a warning when app-server started before the latest config/catalog write. The corrective action is to fully exit all Codex Desktop/app-server processes and reopen Codex before judging the picker.

## 2026-06-14 Codex MultiRouter stable history bucket and 3.16.2-6 export

- Follow-up fix: `sync_codex_history_provider_bucket_to_multirouter` must target `CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID` (`codex_model_router_v2`), not `custom`. `custom` is now treated as a legacy/source bucket along with `openai` and `cc_switch_codex_router`; otherwise explicit history sync can move sessions away from the current MultiRouter runtime bucket and make history disappear again.
- MultiRouter diagnostics now classify provider buckets as `stable_router`, `legacy_router`, `custom`, `builtin_openai_local_base`, or `other`; only `codex_model_router_v2` is pass, legacy/custom are warn, and built-in `openai` pointing at local base is fail.
- Version bumped from `3.16.2-5` to `3.16.2-6` to avoid overwriting a running `3.16.2-5` raw exe during export. New export artifacts: raw exe `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti\windows\raw-exe\CCSwitchMulti_3.16.2-6_x64.exe` SHA256 `B72790130A30692D2BB83BA68B12F4BE05DD2DEAA62F0327A49DF854E40C2231`; installer `...\installer\CCSwitchMulti_3.16.2-6_x64-setup.exe` SHA256 `70A2D0B1BF7772AF9F5D01EC7C934074577B61A64046D10D1B067D5B86CB2D2B`.

## 2026-06-14 Codex Desktop current history repair built into CCSwitchMulti

- Historical note now superseded by 2026-06-23 evidence: one 26.609 local repair succeeded against `~/.codex/sqlite/state_5.sqlite`, but that path must not be treated as the universal default. Current automatic detection prefers configured sqlite homes, then `~/.codex/state_5.sqlite`, with the sqlite subdir kept only as a compatibility fallback.
- CCSwitchMulti now exposes a full `repair_codex_history_visibility` Tauri command and a MultiRouter page button labeled "修复历史显示". The UI first runs `dryRun=true`, shows the active DB/provider/user-event/index/hints/projectless/focus/mtime counts, then asks for explicit confirmation before apply.
- The Rust repair path targets `codex_model_router_v2` by default, treats `openai`, `custom`, `cc_switch_codex_router`, `codex_model_router`, and collected trusted legacy ids as source buckets, and does not switch MultiRouter runtime back to built-in `openai`.
- The repair is broader than provider bucket sync: it resolves the active Desktop sqlite DB, rewrites provider buckets, updates rollout first-line metadata, backfills `has_user_event` from rollout user messages, appends/moves `session_index.jsonl`, repairs `.codex-global-state.json` workspace hints, removes repaired ids from `projectless-thread-ids`, optionally saves/focuses a project root, and touches focused rollout mtimes.
- Regression coverage: `active_state_db_prefers_codex_root_over_sqlite_subdir`, `active_state_db_falls_back_to_sqlite_subdir`, and `repairs_current_desktop_history_visibility_end_to_end` cover root-default detection, sqlite-subdir fallback, `\\?\` cwd normalization, provider/user-event repair, session index append/move, workspace hints/projectless cleanup, saved root insertion, rollout first-line rewrite, and mtime touch.

## 2026-06-14 Codex MultiRouter history repair UI module

- The history repair trigger is no longer only a hidden status-page action. `CodexRouterWorkspacePage` has a dedicated `history` tab labeled `历史修复`, plus a top header shortcut and a status-page shortcut that only navigate to that tab.
- The `历史修复` tab replaces the old `window.prompt` flow with an optional project-root input, `预览修复`, and `确认写入`. Apply is disabled until the current project path has a matching dry-run preview, so changing the path cannot accidentally reuse stale counts.
- The tab surfaces the real backend repair evidence: current MultiRouter plan, Codex takeover state, returned `targetProvider`, active DB path/kind, live config provider, source buckets, visible window counts, backup dir, skipped reason, and per-area counts for provider/user-event/session_index/workspace hints/projectless/focus/mtime/saved roots.
- MultiRouter route editing jitter/display cut-off was traced to the nested route editor dialog in `CodexFormFields.tsx`: content used scroll classes without a stable flex-height parent. The dialog now has `max-h-[90vh] overflow-hidden`, and its body is `flex-1 min-h-0 overflow-y-auto`, so long route forms scroll inside the modal instead of resizing the viewport.

## 2026-06-15 CCSwitchMulti 3.16.2-18 GitHub release

- After adding the dedicated history repair tab, do not reuse the existing `v3.16.2-17` release because that tag points at older commit `02bd8a2a`. The release commit for the history repair UI module is `257e4e54`, tagged and pushed as `v3.16.2-18` on `BigStrongSun/cc-switch`.
- Published GitHub Release: `https://github.com/BigStrongSun/cc-switch/releases/tag/v3.16.2-18`, marked Latest. Uploaded Windows assets: `CCSwitchMulti_3.16.2-18_x64-setup.exe`, `CCSwitchMulti_3.16.2-18_x64-portable.zip`, `CCSwitchMulti_3.16.2-18_x64.exe`, and `SHA256SUMS-v3.16.2-18.txt`.
- SHA256: setup exe `23A5D89CE4C80C78AFC5A55CD7EDA7EAF8DB22BA07B58F1FF8468A0C9FF6B707`; portable zip `C686C1048F5DE1000ABC1D553F6572C72490A09CA0ECB8CD5C0255D965D5B0B9`; raw exe `E0982F380BD44C45EFD1C22AB20208708A4DCDE6CC0AC562453F31999A489E36`.
- `pnpm release:export` succeeded for `3.16.2-18`, but the export root could not clear an old locked `CCSwitchMulti_3.16.2-17_x64.exe`. Future release uploads should stage only the exact target-version assets and a version-specific checksum file instead of uploading the export root wholesale.
- The fork currently shows no GitHub Actions runs after the tag push, so additional Linux/macOS assets will not appear automatically unless Actions are enabled/fixed or those platforms are built and uploaded separately.

## 2026-06-15 CCSwitchMulti 3.16.2-18 Linux release assets

- To match the existing `v3.16.2-17` release shape, Linux x86_64 packages were built in WSL from clean tag `v3.16.2-18` using `pnpm tauri build --bundles appimage,deb,rpm --config <no-updater-artifacts>`. The first attempt failed because the background PATH omitted `~/.cargo/bin`; after adding `/home/openclaw/.cargo/bin`, the build completed.
- Uploaded additional GitHub Release assets to `https://github.com/BigStrongSun/cc-switch/releases/tag/v3.16.2-18`: `CCSwitchMulti_3.16.2-18_amd64.AppImage`, `CCSwitchMulti_3.16.2-18_amd64.deb`, and `CCSwitchMulti-3.16.2-18-1.x86_64.rpm`. `SHA256SUMS-v3.16.2-18.txt` was replaced with a combined Windows+Linux checksum file.
- Linux SHA256: AppImage `011B242C77A870086F684F96842755877E824D57D2C7A1F8B78AA4781C9EBC7A`; deb `730DDD58EA2D72347E7E2CAA987443D5390B43FC6C03D523433B4E95B9DDDDD8`; rpm `232D9CF6E4376BE315B332D06C90661F723C2B24152B0222DCFCD2366B01AF0B`.
- GitHub release verification after upload shows 7 assets total: Windows setup/portable/raw exe, Linux AppImage/deb/rpm, and the combined checksum file. macOS was not produced in this Windows/WSL pass because it needs a macOS runner plus Apple signing/notarization credentials, and the fork still does not expose runnable Actions via `gh workflow list`.

## 2026-06-15 README fork-positioning update

- `README.md` now opens as `CCSwitchMulti` instead of plain upstream `CC Switch`, and the top version/download badges point to the fork release page `BigStrongSun/cc-switch`.
- A new front matter section, `CCSwitchMulti Branch Notice`, explains that this repository is a downstream branch of official CC Switch and that the remaining README still contains inherited upstream documentation.
- The branch notice documents the fork-specific Codex features: `OpenAI Multi-Model Router`, `settings_config.modelCatalog`, `settings_config.codexRouting`, stable `codex_model_router_v2` runtime bucket, Codex Desktop picker unlock/Statsig filtering diagnostics, history visibility repair, and the external OpenAI-compatible API sidecar.
- The usage notes intentionally warn that catalog visibility is not the same as upstream request success, Codex Desktop may need a full restart or CCSwitchMulti unlock flow, picker unlock is runtime renderer injection rather than an on-disk `app.asar` patch, router-owned TOML must not be placed in shared Codex common config, MultiRouter must not be routed through built-in `openai`/`openai_base_url`, and the Codex takeover port is distinct from the sidecar API port.

## 2026-06-15 CCSwitchMulti 3.16.2-19 fork updater and standalone Codex history repairer

- The updater must use the fork release feed, not upstream `farion1231/cc-switch`: `src-tauri/tauri.conf.json` now points to `https://github.com/BigStrongSun/cc-switch/releases/latest/download/latest.json`, and the fallback update page plus About links point to `BigStrongSun/cc-switch`.
- The standalone Codex history repairer is a Windows GUI binary declared as `codex-history-repairer` in `src-tauri/Cargo.toml` behind the `history-repairer` feature. Keep `autobins = false`, otherwise Tauri can accidentally bundle the helper as the main app.
- The GUI calls `repair_codex_history_visibility_standalone`, which reads the live `~/.codex/config.toml` top-level `model_provider` when the target provider field is empty, falls back to `codex_model_router_v2`, auto-detects the active state DB with configured sqlite homes before the default `~/.codex/state_5.sqlite` and legacy sqlite-subdir fallback, and uses source buckets `openai`, `custom`, `codex_model_router_v2`, `cc_switch_codex_router`, and `codex_model_router`.
- Write mode blocks while Codex Desktop/app-server is running unless the GUI force option is enabled. This is intentional because current Desktop can rewrite `.codex-global-state.json` and SQLite WAL state during repair.
- Export script `scripts/export-latest-ccswitchmulti.ps1` now builds the helper with `cargo build --bin codex-history-repairer --features history-repairer --release`, copies it under `tools/codex-history-repairer`, manually signs the NSIS setup with `~/.ccswitchmulti/tauri-update.key`, writes `latest.json`, and stages release assets from the versioned export directory.
- Published release: `https://github.com/BigStrongSun/cc-switch/releases/tag/v3.16.2-19`. Required assets are present: Windows setup, setup signature, portable zip, raw exe, standalone `CodexHistoryRepairer_3.16.2-19_x64.exe`, `latest.json`, notes, and `SHA256SUMS.txt`.
- Verification performed for this release line: `pnpm typecheck`, `cargo fmt --manifest-path src-tauri\Cargo.toml --check`, `cargo test --manifest-path src-tauri\Cargo.toml standalone_repair_defaults_target_to_live_config_provider --lib`, `cargo test --manifest-path src-tauri\Cargo.toml repairs_current_desktop_history_visibility_end_to_end --lib`, and `cargo build --manifest-path src-tauri\Cargo.toml --bin codex-history-repairer --features history-repairer --release`.

## 2026-06-15 Codex history repair latest-script parity

- User screenshot showed `v3.16.2-19` still did not surface all repaired sessions. Root cause: the built-in Rust repair and `repair-codex-history-current-desktop.ps1` reproduced active DB/provider/user-event/index/hints/focus/mtime, but missed the later successful `balance-codex-history-recent-window.ps1 -MaxPerProject 10 -MaxTotal 300 -SourceFilter vscode -SyncRolloutMtime` step. Codex Desktop first takes a limited global recent thread window and only then groups by workspace, so current-project focus alone can still leave sessions outside the sidebar window.
- The repair backend now supports `balance_recent_window`, `max_per_project`, `max_total`, and `source_filter`. Visibility filtering uses the provider after planned bucket migration, so rows currently under `openai/custom/legacy` are counted before the write instead of disappearing from the preview.
- The balanced repair keeps the current project focus count as a floor, then round-robins remaining visible rows by normalized `cwd` with per-project and total caps. The MultiRouter history tab and standalone GUI default to `sourceFilter="vscode"`, `maxPerProject=10`, and `maxTotal=300` to match the successful Desktop-sidebar repair path.
- The rollout metadata repair now scans all JSONL lines with `payload.model_provider`, not only the first `session_meta` line, and restores the previous rollout file mtime after provider metadata rewrite; only the explicit focus/balanced mtime step changes sidebar ordering.
- `session_index.jsonl` repair now overwrites stale `thread_name` for selected rows and reports `sessionIndexTitles*` counts. Regression tests cover provider-after visibility, multi-project recent-window balancing, source filter behavior, stale title overwrite, and multi-line rollout provider rewrite.
- Verification passed: `cargo test --manifest-path src-tauri\Cargo.toml codex_history_migration::tests --lib -- --nocapture`, `cargo fmt --manifest-path src-tauri\Cargo.toml --check`, `pnpm typecheck`, and `cargo build --manifest-path src-tauri\Cargo.toml --bin codex-history-repairer --features history-repairer --release` (existing unrelated `commands/misc.rs` dead_code warnings only).

## 2026-06-15 CCSwitchMulti 3.16.2-20 history repair productization

- The current productized history-repair baseline is the latest successful balanced-window flow, not the older provider-only repair: active DB resolution must auto-detect configured sqlite homes, then default `~/.codex/state_5.sqlite`, then legacy `~/.codex/sqlite/state_5.sqlite`; repair targets must follow live `config.toml` or `codex_model_router_v2`, and the default visibility path is `sourceFilter="vscode"`, `maxPerProject=10`, `maxTotal=300`, with rollout mtime sync.
- CCSwitchMulti now adds `list_codex_history_sessions` and extends `repair_codex_history_visibility` with `codexHome`, `stateDbPath`, and `sessionIds`. The history tab can set Codex home, list active SQLite session summaries, search/filter records, select specific sessions for targeted recovery, or leave selection empty to run the balanced project/global recent-window repair.
- The Rust repair runtime treats nonempty `sessionIds` as an explicit focus set: provider/user-event repair still covers visible candidates, but focus movement, session_index move, workspace hints, and rollout mtime touch only apply to selected sessions; balanced recent-window reporting is disabled in that targeted mode. Regression coverage: `selected_session_ids_focus_only_requested_rows`.
- Standalone delivery is no longer a Windows GUI exe in the export pipeline. `scripts/codex-history-tool/codex_history_tool.py` is a standard-library Python tool with `list` and `repair` subcommands, exported under `tools/codex-history-tool` with README; `scripts/export-latest-ccswitchmulti.ps1` no longer builds or copies `codex-history-repairer.exe` and excludes `__pycache__`/`.pyc`.
- Version bumped to `3.16.2-20`; `pnpm release:export` produced `CCSwitchMulti_3.16.2-20_x64-setup.exe`, `.sig`, portable zip, raw exe, `latest.json`, and the Python history tool in `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`. The export still warned that an old `CCSwitchMulti_3.16.2-17_x64.exe` was locked, but the target-version artifacts and tool checksums were written.
- Verification passed: `python -m py_compile scripts\codex-history-tool\codex_history_tool.py`, Python `list --limit 3 --json`, Python repair dry-run for `C:\Users\sunda\Documents\LLMservice`, `cargo check --manifest-path src-tauri\Cargo.toml --lib`, `cargo test --manifest-path src-tauri\Cargo.toml codex_history_migration::tests --lib -- --nocapture`, `cargo fmt --manifest-path src-tauri\Cargo.toml --check`, `pnpm typecheck`, `pnpm history:tool:check`, and `pnpm release:export`.

## 2026-06-16 CCSwitchMulti Codex history repair moved into Session Manager

- Supersedes the 2026-06-14 MultiRouter history tab placement: the product UI for Codex history repair now belongs in `src/components/sessions/SessionManagerPage.tsx` behind the Codex-only FileClock toolbar button, not in `CodexRouterWorkspacePage.tsx`. The MultiRouter workspace page no longer exposes a history repair tab/button and its old inline repair component was removed to prevent reviving stale provider-only UI.
- The built-in repair flow is implemented by `src/components/sessions/CodexHistoryRepairPanel.tsx`. It keeps the latest successful baseline defaults (`sourceFilter="vscode"`, `maxPerProject=10`, `maxTotal=300`, balanced recent window, auto-detected active state DB, rollout mtime sync), adds light default path hints, source/provider count panels, target-provider dropdown candidates, and SQLite-backed session selection.
- The Tauri backend now exposes `read_codex_history_session` so the Session Manager repair panel can inspect a selected SQLite session by following `threads.rollout_path` and parsing the local Codex JSONL into existing `SessionMessage` rows. `list_codex_history_sessions` also returns `sourceCounts`, `providerCounts`, and `targetProviderCandidates`.
- Built-in `repair_codex_history_visibility_for_multirouter` now matches the standalone/Python behavior when `targetProvider` is empty: prefer live `~/.codex/config.toml` top-level `model_provider`, then fall back to `codex_model_router_v2`. This avoids repairing the active third-party provider's history back into official `openai`.
- Regression coverage added: `multirouter_repair_defaults_target_to_live_config_provider`, `list_history_sessions_returns_provider_source_candidates_and_all_sources`, and `read_history_session_loads_rollout_messages_from_sqlite_path`. Verification passed: `cargo test --manifest-path src-tauri\Cargo.toml codex_history_migration::tests --lib -- --nocapture`, `cargo fmt --manifest-path src-tauri\Cargo.toml --check`, targeted Prettier check for changed frontend files, `pnpm typecheck`, and `pnpm build:renderer`.

## 2026-06-16 CCSwitchMulti 3.16.2-21 provider edit route stability

- MultiRouter provider edit page route rows disappearing/jittering was traced to frontend state timing, not backend route persistence: `useCodexConfigState` initialized Codex catalog/routing to empty and only filled them in an effect, while `CodexFormFields` could echo the first-frame empty child rows back to the parent and overwrite loaded routes.
- The fix initializes auth/config/baseUrl/catalog/spawnAgent/routing synchronously from `initialData`, keeps prop-change keys for catalog/routing, and skips child-to-parent echo during external provider loads until local rows match the incoming state. The route list now keeps a stable empty-state container instead of collapsing, and the duplicate local-routing toggle was removed from Advanced Options.
- The wrong MultiRouter-page history-repair link remains removed; Codex history repair stays in Session Manager behind the Codex-only FileClock entry, using the 2026-06-15 balanced recent-window repair baseline.
- Export script hardening: `scripts/export-latest-ccswitchmulti.ps1` now copies only the current setup artifact's `.sig`, so stale signatures from older bundle outputs cannot leak into `SHA256SUMS.txt` or release staging.
- Version `3.16.2-21` was built/exported. Clean export path: `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.2-21`. The normal `最新版ccswitchmulti` export path also received target-version artifacts, but an already running `CCSwitchMulti_3.16.2-20_x64.exe` kept old files locked, so the clean release handoff should use the versioned export directory.
- Verification passed: `pnpm typecheck`, `pnpm build:renderer`, `cargo check --manifest-path src-tauri\Cargo.toml --lib`, `cargo fmt --manifest-path src-tauri\Cargo.toml --check`, locale JSON parse check, `git diff --check`, and full `pnpm release:export` plus clean `-SkipBuild` export. Browser dev-mode UI inspection confirmed the Codex add-provider form has the new route empty state, no old Advanced hint, no visible MultiRouter history-repair link, and the expected local routing control; true desktop v21 UI inspection was blocked by Tauri single-instance because v20 was still running.

## 2026-06-17 MultiRouter spawn_agent candidate ordering placement

- `settingsConfig.modelCatalog.spawnAgentModels` is route/catalog configuration, so the MultiRouter candidate ordering UI belongs in `CodexRouterWorkspacePage` RoutesTab, not StatusTab.
- The route-rule panel copy should state that the first 5 models are the Codex `spawn_agent` visible models and can be drag-sorted. Both the preview window and the sortable draft list should visually highlight those first five candidates.
- StatusTab should not expose candidate editing controls (`保存排序`, `校验候选`, drag list, candidate source tabs). Keep it focused on link readiness, diagnostics, provider targets, traffic, router logs, and model-picker unlock evidence.

## 2026-06-17 MultiRouter workbench dedupe and External API multi-key credentials

- The MultiRouter top workbench should stay compact and action-oriented. Keep only the short positioning text plus create/manage/status navigation buttons there; link readiness, local listener, Codex takeover, enabled rules, diagnostics, traffic attribution, router logs, and picker evidence belong in StatusTab.
- Do not revive the removed "操作记录" tab, and do not move `modelCatalog.spawnAgentModels` editing back into StatusTab. Candidate ordering remains route/catalog configuration under RoutesTab after commit `057b43f7`.
- External OpenAI-compatible / third-party Agent API credentials now support multiple local `ccsw_` keys. New profile JSON stores key records in `apiKeys` with id, plaintext local sidecar key, prefix, and created_at so the UI can list, copy again later, and delete old keys.
- Compatibility boundary: `api_key_hash` / `api_key_prefix` are still maintained for the latest generated key and legacy hash-only profiles. A legacy profile with only hash material is shown as a non-copyable legacy key because plaintext was never stored; it can still be deleted. Deleting the last new-format key must also clear the compatibility hash so a removed key cannot continue authenticating.
- Security boundary: the reusable plaintext key is only the CCSwitchMulti-generated local `ccsw_` sidecar credential. Upstream OAuth tokens, refresh tokens, and real provider API keys are not exposed through the External API credentials page.

## 2026-06-16 CCSwitchMulti Session Manager history repair primary layout

- User feedback after the Session Manager move: the Codex history repair entry was still too hidden and the repair UI looked like an awkward utility panel. The product decision is now stronger: when `SessionManagerPage` is opened for Codex, history repair is the default primary workspace, with an explicit two-button switch for `历史修复` and `会话浏览` in the session list header.
- `CodexHistoryRepairPanel` now presents a single repair workbench instead of stacked cards: top action bar for load/preview/apply, status tiles for active DB / loaded-selected count / write state, a compact horizontal path-and-scope settings band, then SQLite history, session JSONL preview, and repair evidence columns. This keeps the latest balanced-window repair defaults visible without making the user hunt for the entry.
- The panel auto-loads active SQLite only when the Tauri runtime is present, so the real desktop app starts with useful history data while browser/dev preview does not show a false `invoke` error.
- Verification passed: targeted Prettier check, `pnpm typecheck`, `pnpm build:renderer`, and Browser dev-mode inspection at `http://127.0.0.1:3000/`. Browser DOM confirmed visible `历史修复` / `会话浏览` buttons, default Codex history repair main area, no development `invoke` error, and no horizontal overflow at 1280 px.

## 2026-06-16 CCSwitchMulti 3.16.2-22 release

- Version bumped to `3.16.2-22` for the Session Manager history-repair layout release. Export root: `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.2-22`.
- Release export verification: `latest.json` reports `3.16.2-22`, `SHA256SUMS.txt` contains only v22 Windows binaries, and the export includes setup exe/signature, portable zip, raw exe alias/versioned exe, platform build notes, README, and `tools/codex-history-tool`.
- Verification before release: targeted Prettier check, `pnpm typecheck`, `pnpm history:tool:check`, `cargo check --manifest-path src-tauri\Cargo.toml --lib`, and `scripts\export-latest-ccswitchmulti.ps1 -ReleaseRoot ...3.16.2-22`. Rust still only reports the existing `commands/misc.rs` dead_code warnings.

## 2026-06-21 MultiRouter route-rule picker

- `CodexRouterWorkspacePage` RoutesTab must not route “编辑匹配规则” into the generic Provider edit form. That form exposes the low-level `codexRouting.routes` editor and the old “添加 route” path can freeze or produce an unusable workflow for MultiRouter rule editing.
- Route-rule editing in the MultiRouter workspace is now an in-page candidate router picker: it merges existing routes with all non-routing Codex model sources, lets the user directly select/enable candidate routers, and writes only `settingsConfig.codexRouting.routes` through `providersApi.update(nextProvider, "codex")`.

## 2026-06-21 MultiRouter provider edit entry

- Codex MultiRouter providers in the main provider list must not open `EditProviderDialog` / generic `ProviderForm`. The generic form is only for normal upstream providers and can still expose the legacy route editor path where “添加 route” freezes.
- Main-list edit, and any workspace edit action for a routing plan, should navigate to `CodexRouterWorkspacePage` with that provider selected and `initialTab="routes"`. The dedicated workspace owns route selection, enabled state, model catalog, and spawn-agent candidate persistence.

## 2026-06-21 WebDAV/S3 sync portability

- WebDAV/S3 database sync must not blindly upload machine-specific absolute paths or keys when sharing a profile across devices. Sync export rewrites the current user home path to `${CC_SWITCH_HOME}` and import localizes that token, plus common `C:\Users\<other>` / `/Users/<other>` / `/home/<other>` paths, to the receiving machine.
- `includeKeysOnUpload` controls whether provider/API/MCP keys remain in the uploaded SQL snapshot. When disabled, key/token/password values are stripped while auth mode and routing structure are preserved so the receiving user can fill their own credentials.
- New route candidates should reference `targetProviderId` and `auth.source="provider_config"` instead of copying API keys or Base URLs. This preserves model-source ownership and keeps the workspace from scattering provider credentials into route rows.
- Verification passed for this change: targeted Prettier write/check on `src/components/codex/CodexRouterWorkspacePage.tsx`, `pnpm typecheck`, `git diff --check`, and `pnpm build:renderer`. Build still reports the existing browserslist/baseline staleness and large chunk warnings only.

## 2026-06-22 CCSwitchMulti v3.16.3-8 merge release preparation

- Purpose: make the next release a full successor by merging the `v3.16.3-5` release line into the `v3.16.3-7` MultiRouter/context-window line, instead of treating `v3.16.3-7` as a standalone targeted prerelease.
- Merge strategy: use a real git merge so the history records both parents. This preserves the official v3.16.3 merge, takeover restore preservation fix, unified history repair safeguards, and the newer MultiRouter/WebDAV/context-window changes.
- Version surfaces for the merged release must be `3.16.3-8` in `package.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, and `src-tauri/tauri.conf.json`.
- Release rule: do not retag or force-update `v3.16.3-7`; publish the merged successor as a new tag/release.

## 2026-06-24 CCSwitchMulti v3.16.3-14 follow-up product fixes

- MultiRouter official/Codex fallback model catalog must carry explicit context windows. When an OpenAI/Codex OAuth provider has no real model catalog, fallback entries should be `gpt-5.5=272000`, `gpt-5.4=272000`, `gpt-5.4-mini=128000`, and `gpt-5.3-codex-spark=128000`; otherwise Codex Desktop can fall back to its 128k-ish display budget and users report GPT-5.5 as only about 122k context.
- The usage dashboard historically had rollup/prune maintenance but no user-triggered "clear logs" product path. The correct clear operation deletes `proxy_request_logs` and `usage_daily_rollups` only, preserving provider records, pricing rows, auth material, and app config.
- Port conflicts on 15721/15722 are real multi-instance/old-process failure modes. The low-risk product fix is to surface an actionable `AddrInUse` diagnostic naming CCSwitchMulti/old process/alternate port; a stronger cross-process singleton lock is separate work and should not be mixed into takeover restore logic casually.
- Codex Desktop model-picker unlock should not treat the CLI `codex.exe` as Desktop. Desktop executable discovery may include WindowsApps package layouts (`app/Codex.exe`, `app/resources/Codex.exe`, package root `Codex.exe`) and `%LOCALAPPDATA%\OpenAI\Codex`, but should avoid launching lowercase CLI paths. Launch should re-check whether Codex Desktop is already running before starting with CDP flags.
- OAuth token dual-store remains a risk boundary, not a solved low-risk fix: `~/.codex/auth.json` and CCSwitchMulti `codex_oauth_auth.json` can diverge. Do not blindly copy managed refresh tokens into Codex Desktop auth as a "sync" fix without proving rotation/account semantics; prefer preserving Codex login material and using managed OAuth only for proxy forwarding/quota paths.

## 2026-06-25 CCSwitchMulti v3.16.3-20 prerelease for MultiRouter model-refresh hang

- User screenshot with "候选 provider 模型列表刷新" cards stuck at "正在读取模型列表..." was a release-boundary issue first: public `v3.16.3-19` points at `6a1cf4e1` and does not include `ddfeed42` / `33a0bc58`, while the fixed local line is `4f1f911c` after `ddfeed42`, `33a0bc58`, and `272d02a3`. Future reports with the same UI should first check installed version/tag before debugging official Responses routing or upstream `/models`.
- Published prerelease `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.3-20`. Annotated tag `v3.16.3-20` dereferences to `4f1f911cae3ea13f78412c720854ab87201ee7c7`; release is non-draft and prerelease=true. Release notes are Chinese and explicitly describe the model-list loading hang, per-provider attempt tracking, API-key-sensitive stale request suppression, 30 second frontend timeout, and visible-model vs upstream-model split.
- Windows assets came from the local export pipeline at `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.3-20` and flat upload staging at `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.3-20-assets`. Raw exe `CCSwitchMulti_3.16.3-20_x64.exe` reports ProductVersion/FileVersion `3.16.3-20`; `RELEASE-METADATA.md` records commit `4f1f911c`.
- Linux assets were built locally in WSL distro `openclaw` from `/home/openclaw/ccswitchmulti-linux-build-v3.16.3-20`, after cloning the fork tag and verifying HEAD equals `4f1f911c`. Commands used Linux Node path `/home/openclaw/.local/node-v22.22.2-linux-x64/bin`, `pnpm install --frozen-lockfile --prefer-offline`, `cargo build --manifest-path src-tauri/Cargo.toml --bin codex-history-repairer --features history-repairer --release`, and `pnpm tauri build --bundles appimage,deb,rpm --config <createUpdaterArtifacts=false>`. The build succeeded; only the final `sha256sum` list command failed from CRLF glob input after artifacts had already been copied to Windows staging.
- macOS universal assets were produced by GitHub Actions run `28169469534` (`supplemental-macos-release.yml`) against `v3.16.3-20`; it completed successfully and uploaded `CCSwitchMulti_3.16.3-20_universal.tar.gz`, `.tar.gz.sig`, and `.app.zip`. This workflow also refreshed `SHA256SUMS.txt`.
- Final release has 12 assets: Windows setup/signature/portable/raw exe, Linux AppImage/deb/rpm, macOS universal tar/signature/app zip, `latest.json`, and `SHA256SUMS.txt`. Final `SHA256SUMS.txt` covers all release assets except itself; GitHub asset digests matched the local Windows/Linux checksums and workflow-produced macOS digests.
- Known non-blocking warnings in this release remain the existing Vite baseline/browserslist/chunk warnings, Rust unused/dead_code warnings, and Tauri `__TAURI_BUNDLE_TYPE variable not found` bundler warning. Fork push still triggers failing generic CI/release workflows, but manual local/WSL build plus supplemental macOS workflow are the verified publishing path for this prerelease.

## 2026-06-28 MultiRouter duplicate visible model semantics

- If OpenAI official and a third-party relay both expose the same visible model id such as `gpt-5.5`, MultiRouter does not infer quality, provider type, price, or freshness. Route order is the control surface.
- Frontend catalog generation in `CodexRouterWorkspacePage::buildModelCatalogForRoutes` uses a `Map` keyed by visible `model`; while iterating routes in order, the first route/source that contributes `gpt-5.5` wins and later same-id entries are skipped. The picker and `spawn_agent` catalog therefore show one `gpt-5.5`, not one per upstream.
- Runtime route resolution in `src-tauri/src/proxy/providers/codex.rs` uses `routes.iter().find(...)` over enabled routes. Exact `match.models` and prefix matches are case-insensitive, but duplicate exact matches still choose the first matching route in the saved `codexRouting.routes` array.
- `defaultRouteId` is only used when no enabled route matches the requested visible model. It does not override a duplicate exact match and does not choose between two `gpt-5.5` routes.
- The public helper `resolve_codex_model_routed_providers` can produce a primary route plus other enabled route candidates for future fallback use, but current forwarder code calls the single-provider wrapper `resolve_codex_model_routed_provider`, which takes `.next()`. Current HTTP routing therefore uses only the first resolved route inside the selected MultiRouter provider; it is not round-robin or automatic same-model failover.
- Upstream model rewriting is separate from route selection. Route `modelMap` / `upstreamModel` / `model` writes `codexResolvedUpstreamModelOverride` and takes priority; otherwise the catalog model's `upstreamModel` can rewrite the outbound body model. If neither exists, the visible request model is preserved for matched routes. For an unmatched fallback-style routed provider, Chat conversion forces the route/provider's own configured model so `gpt-5.5` is not blindly sent to DeepSeek/Qwen.
- Recommended configuration when a third-party relay provides an upstream named `gpt-5.5`: use a distinct visible alias such as `gpt-5.5-relay` with `upstreamModel="gpt-5.5"` if users need both official and relay selectable at the same time. If both are intentionally the same visible `gpt-5.5`, put the desired primary route first and treat the duplicate as shadowed unless the route order is changed.
- Live diagnosis on 2026-06-28: both `codex-multirouter` and `codex-openai-router` had official GPT routes before aggregate-platform routes, with broad prefixes (`gpt` or `gpt-`). A request for `gpt-5.5-pro` therefore matched the official route by prefix and was sent to ChatGPT Codex OAuth, producing "model is not supported when using Codex with a ChatGPT account" instead of using a third-party relay. The locally configured aggregate provider `yansd666带gpt官方模型` did not expose `gpt-5.5-pro` in `/models`; direct `/responses` and `/chat/completions` calls with `model=gpt-5.5-pro` both returned HTTP 503 "无可用渠道", so that provider currently supports `gpt-5.5` but not `gpt-5.5-pro`.
- When a MultiRouter route references `targetProviderId`, `materialize_codex_routed_provider_from_target` deliberately follows the target provider's `base_url`, auth, and `apiFormat`; the route row only carries route identity/capabilities/model override. For an aggregate platform that mixes native Responses-compatible GPT models with Chat-Completions-only third-party models, use separate provider entries or route-level inline upstreams per protocol. Do not rely on one global "需要本地路由映射" switch to represent both protocols at once.

## 2026-06-28 MultiRouter route-rule picker duplicate provider fix

- Editing an old MultiRouter after adding new normal providers can show duplicate Qwen/DeepSeek rows when the saved route is legacy/inline and the new provider-backed candidate has the same semantic model source. The root is not backend routing: the workspace candidate builder only deduped by `targetProviderId`, while legacy routes may have no target or may have lost `route.provider` during `normalizeLegacyCodexRoutingRoute`.
- The frontend fix is to preserve legacy `route.provider` / `upstream.provider` as `targetProviderId`, and to dedupe routes by semantic provider before rendering route entries, building candidate picker rows, and saving `codexRouting.routes`. Semantic matching falls back to normalized provider name/id and model/prefix overlap only when no explicit target provider exists.
- New provider candidates in `RouteCandidatePicker` should be directly actionable: clicking the right-side `启用` button on an unchecked candidate now selects and enables it in one step. Do not reintroduce `disabled={!checked || isSaving}` for that button, or users will again need to click `全选并启用` before adding one provider.
- Regression coverage lives in `src/components/codex/CodexRouterWorkspacePage.test.ts`: legacy provider references are preserved/deduped, and a new provider candidate can be enabled and saved without using global select-all. Verified with `.\node_modules\.bin\vitest.cmd run src/components/codex/CodexRouterWorkspacePage.test.ts`, `.\node_modules\.bin\tsc.cmd --noEmit`, and targeted Prettier check.

## 2026-06-28 MultiRouter gpt-5.5-pro source boundary

- When investigating a report that `gpt-5.5-pro` was "fetched", first distinguish model catalog acquisition from Codex runtime request input. The user screenshot of the yansd666 provider's model mapping showed `gpt-5`, `gpt-5-codex`, `gpt-5.1`, `gpt-5.1-codex`, `gpt-5.3-codex-spark`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.5`, and `gpt-image-2`; it did not show `gpt-5.5-pro`.
- For a new empty yansd666 Codex provider, clicking "获取模型列表" can still populate those official-looking GPT ids because `https://yansd666.com/v1/models` itself returns exactly those 9 ids for the configured account/key. Direct checks with default UA and a browser-like Mozilla UA both returned the same 9 ids and did not return `gpt-5.5-pro`.
- Live DB verification on `~/.cc-switch/cc-switch.db` found no `gpt-5.5-pro` string in current providers, including `yansd666带gpt官方模型` and the active `codex-openai-router`. `~/.codex/cc-switch-model-catalog.json` and `~/.codex/models_cache.json` also did not contain `gpt-5.5-pro`; the only live `~/.codex/state_5.sqlite` hits were Codex thread/task text created while debugging the screenshot.
- The model fetch path is literal: `fetchModelsForConfig` calls Tauri `fetch_models_for_config`, which calls `src-tauri/src/services/model_fetch.rs::fetch_models`; the backend parses OpenAI-compatible `/models` entries into `FetchedModel.id` and sorts them, without synthesizing `-pro` suffixes. Frontend merge paths in `CodexFormFields` and `CodexRouterWorkspacePage::providerWithFetchedModelCatalog` add fetched ids as `{ model: id, upstreamModel: id, displayName: id }`, and also do not generate `gpt-5.5-pro`.
- `providerWithFetchedModelCatalog` is additive: it updates context windows and appends new remote ids but does not prune models that disappeared from `/models`. Therefore a stale `gpt-5.5-pro` can persist on another user's machine if their provider catalog previously contained it or they manually added it, but that was not the state on this machine during the 2026-06-28 check.
- The observed toast saying `model: gpt-5.5-pro` was a runtime request boundary: Codex Desktop sent `/responses` with `model=gpt-5.5-pro`; the then-current router matched broad official GPT prefixes before the later aggregate route and sent it to ChatGPT Codex OAuth. After commit `bbe9d93d`, exact route matches take precedence globally over earlier prefixes, but if no exact `gpt-5.5-pro` route/catalog exists, a broad prefix can still be the intended fallback behavior.

## 2026-06-28 Codex official login preservation on provider switch

- The user-facing bug "switch provider, then official Codex asks to log in again" is a non-takeover `auth.json` overwrite problem, not an official-login bypass problem. Before this fix, `codex_config::write_codex_live_for_provider` still wrote non-official provider `auth.OPENAI_API_KEY` into `~/.codex/auth.json` when `preserve_codex_official_auth_on_switch=false`, and switching back to official could write a stale DB OAuth snapshot over the current live OAuth auth.
- Correct rule: third-party Codex provider switches should always leave `~/.codex/auth.json` alone and place the provider/API/proxy bearer in `config.toml` as `experimental_bearer_token`. Official switches should only write `auth.json` when live auth does not already contain real OAuth login material; if live auth has OAuth tokens, only refresh `config.toml`.
- Keep `codex_auth_has_oauth_login_material` separate from `codex_auth_has_login_material`: `OPENAI_API_KEY` is a provider token, not official login material. Do not treat third-party bearer keys as a reason to preserve/overwrite official OAuth auth.
- Regression coverage: `third_party_live_write_preserves_existing_codex_oauth_auth`, `official_live_write_preserves_current_oauth_auth_over_stale_db_snapshot`, updated `codex_custom_provider_live_write_preserves_oauth_auth_even_when_preserve_disabled`, plus existing takeover official return test `codex_switch_to_official_during_takeover_exits_proxy_and_cleans_router_fields`. Verified with targeted `cargo test --manifest-path src-tauri\Cargo.toml ... --lib` and `cargo fmt --manifest-path src-tauri\Cargo.toml --check`.

## 2026-07-03 CCSwitchMulti v3.16.4-7 release-note rewrite from tag diff

- User asked for a rewritten `v3.16.4-7` release note that compares the latest release to `v3.16.4-4` from actual per-version commits, not commit notes. The correct analysis boundary is tag diff, with `v3.16.4-7` tag `755b69e4` as the release point; later memory-only commits `5dbb8cd7` and `e8935a46` are post-release records and should not be treated as product changes.
- Diff-derived version summary: `v3.16.4-5` adds Codex Desktop OAuth login preservation during snapshot/backup restore, official same-account vs cross-account auth handling, MultiRouter wizard naming/model-selection/spawn-agent steps, provider-edit-to-MultiRouter catalog/route/spawn-agent synchronization, concurrent Codex protocol probing, release workflow hardening, and OAuth/request-shape diagnostics. `v3.16.4-6` only fixes official Codex OAuth Responses input items by removing invalid `content` from non-message/non-reasoning items. `v3.16.4-7` fixes MultiRouter duplicate GPT aliases, empty target catalogs wiping relay routes, non-routable aggregate catalog models, Volcengine AgentPlan model listing via `ListArkAgentPlanModel`, sensitive image retry, and Codex Responses control-message promotion.
- Release note rule learned: write this release as a cumulative `v3.16.4-4 -> v3.16.4-7` user-facing changelog grouped by impact, then include a short per-version section for traceability. Do not list `memory.md`, docs-only release files, function visibility changes, `parse_context_tokens` cleanup, or log wording changes as product updates.
- Evidence files used for the rewrite: `src/lib/codexMultiRouterSync.ts`, `src/components/codex/CodexMultiRouterWizard.tsx`, `src/components/codex/CodexRouterWorkspacePage.tsx`, `src-tauri/src/codex_config.rs`, `src-tauri/src/services/provider/live.rs`, `src-tauri/src/services/proxy.rs`, `src-tauri/src/proxy/providers/openai_compat.rs`, `src-tauri/src/proxy/forwarder.rs`, `src-tauri/src/proxy/media_sanitizer.rs`, `src-tauri/src/services/model_fetch.rs`, `src/utils/codexPlanModelFetch.ts`, and `.github/workflows/release.yml`.

## 2026-07-06 Codex reset credits watcher integration

- `jordan-edai/codex-reset-watcher` is useful as a reference for Codex banked reset credits, but its macOS SwiftUI/MenuBar layer should not be copied. The portable core is: read the same Codex OAuth login context, call `GET https://chatgpt.com/backend-api/wham/usage` and `GET https://chatgpt.com/backend-api/wham/rate-limit-reset-credits`, and keep the feature strictly read-only.
- CCSwitchMulti already had cross-platform Codex quota plumbing in `src-tauri/src/services/subscription.rs`: macOS can read Keychain `Codex Auth`, while Windows/Linux fall back to `~/.codex/auth.json` through `codex_config::get_codex_auth_path()`. The correct integration point is extending `SubscriptionQuota`, not adding a macOS-specific watcher clone.
- The implemented reset-credit response intentionally exposes only safe display fields: available count, reset type, status, expiry, and title. Do not surface raw endpoint JSON, credit ids, account ids, user ids, access tokens, refresh tokens, or auth file paths in frontend state, logs, or saved snapshots.
- Codex `/wham/usage` `reset_at` can be seconds or milliseconds. `unix_ts_to_iso` now normalizes millisecond epochs and rejects implausible dates outside 2020-2100; keep this guard if endpoint parsing is refactored.
- Partial failure rule: if `/wham/rate-limit-reset-credits` fails but `/wham/usage` succeeds, quota should remain successful and may fall back to `rate_limit_reset_credits.available_count` from the usage response while exposing a reset-credit-specific error. Do not mark the whole quota query failed unless the primary usage query fails or credentials are invalid.
- Verification for this integration: `cargo test --manifest-path src-tauri\Cargo.toml codex_reset_ --lib`, `cargo check --manifest-path src-tauri\Cargo.toml`, `cargo fmt --manifest-path src-tauri\Cargo.toml --check`, and `pnpm typecheck`.

## 2026-07-07 Codex Built-in Web Search vs Third-party Router Boundary

- 用户追问为什么当前主 Agent 能用 Codex 内置 web search，而 DeepSeek V4 子会话/第三方模型说找不到内置搜索工具。排查结论：至少有三类不同能力不能混为一谈：当前 Chat/Codex 编排环境的系统工具 `web.run`、Codex CLI/App 的 first-party Responses `web_search`、以及用户配置的 `matrix-websearch` MCP 函数工具。`web.run` 是本会话系统工具，不会出现在 `tool_search` 或第三方模型的 MCP 工具列表里。
- 官方 Codex 手册显示稳定配置是顶层 `web_search = "cached" | "live" | "disabled"`，其中 `cached` 是默认，`--search` 等价于 `web_search = "live"`；`[features].web_search*` 是遗留开关。`codex features list` 在本机显示 `search_tool`/`tool_search` 已 removed，`standalone_web_search` 仍是 under development，不应作为稳定解决方案。
- 本机 `~/.codex/config.toml` 当前走 `model_provider = "codex_model_router_v2"`，`base_url = "http://127.0.0.1:15721/v1"`，`wire_api = "responses"`，当前模型是 `deepseek-v4-flash`。`codex debug models` 和 `~/.codex/cc-switch-model-catalog.json` 都显示 DeepSeek 条目有 `supports_search_tool = true`、`web_search_tool_type = "text"`，但这只是模型目录/能力描述，不等于本地代理已经能执行 OpenAI 托管的 first-party web search。
- 运行验证：`codex exec --json --skip-git-repo-check -m deepseek-v4-flash -c 'web_search="live"' ...` 没有产生官方手册所说的 `web_search` transcript item；模型反而通过 `tool_search` 懒加载并调用了 `matrix-websearch` MCP。即使额外 `--enable standalone_web_search`，DeepSeek 仍表示没有直接的内置 web_search 工具，只是读文档/本地文件回答。这说明当前第三方路由路径不是单纯缺少顶层 `web_search` 配置，而是 first-party web_search 没有作为稳定可调用工具注入到该第三方模型执行路径。
- 代码边界：`transform_codex_chat.rs` 已经桥接了 Codex `tool_search`，包括把 `{"type":"tool_search"}` 转为 Chat function `tool_search`，并能把上游 Chat tool call 恢复为 `tool_search_call`；但没有同等桥接 OpenAI 托管的 first-party `web_search`。不要把 `web_search_preview` 之类 OpenAI 原生托管工具简单转换为第三方 Chat function，除非同时实现可执行的本地搜索 runner 和 Codex 可接受的返回协议；否则模型只会生成一个没有宿主执行器的函数调用。
- 推荐方案：官方/OpenAI 模型需要内置搜索时，用 `web_search = "live"` 或交互式 `codex --search`，并走官方 OpenAI/OAuth 路径；第三方 DeepSeek/Qwen/GLM 等模型需要联网检索时，稳定方案是走 MCP（例如 `matrix-websearch`），通过 `tool_search` 懒加载或直接暴露 MCP search/open/find 工具。产品层如要修正体验，应把 UI/说明改成“OpenAI first-party web_search”和“MCP web search”两条能力，而不是让第三方 catalog 的 `supports_search_tool=true` 暗示它一定能用 OpenAI 托管搜索。
- 进一步确认：OpenAI first-party web_search 是 hosted tool，执行点在 OpenAI-hosted Responses API 服务端；只要请求路径变成本机 `codex_model_router_v2` / 第三方 OpenAI-compatible provider，OpenAI-hosted Responses API 就不在请求链路里，第三方模型不能“直接调用”这个托管搜索。官方 Bedrock 文档也明确说非 OpenAI-hosted provider 下依赖 OpenAI-hosted cloud services / hosted tools 的功能不可用。
- 想让第三方模型“具备同等搜索能力”只有两条现实路线：一是不要走第三方 provider，把需要 first-party web_search 的任务切到官方 OpenAI/OAuth provider；二是在 Codex/CCSwitchMulti 侧实现本地工具执行器，把搜索做成 MCP/function tool（可复用 `matrix-websearch` 或另写 OpenAI web-search 查询服务），由第三方模型发普通工具调用，本地执行后把结果回灌给模型。这是“自建搜索工具桥接”，不是让第三方直接使用 OpenAI hosted web_search。
- 抓包复核：用本地假 `openai_base_url = "http://127.0.0.1:18081/v1"` 捕获官方 provider + `web_search="live"` 的 `POST /v1/responses`，Codex 先尝试 `ws://.../v1/responses`，失败后回退 HTTP SSE；请求体默认 `content-encoding: zstd`，可用 zstd 解压。实际 `tools` 数组里同时有 client `tool_search` 和 hosted `{"type":"web_search","external_web_access":true,"search_content_types":["text","image"]}`，`tool_choice="auto"`、`stream=true`、`store=false`、`parallel_tool_calls=true`。这说明官方搜索的触发入口确实是 Responses 请求里的 hosted `web_search` tool 声明。
- 代理伪造方案边界：可以在 CCSwitchMulti 拦截 `/v1/responses`，识别并移除/替换 `type=web_search`，把它转成第三方可见的 function tool（例如 `name="web_search"`，参数 `{query, count}`），由代理执行 `matrix-websearch` 或其它搜索 runner，再把结果作为 tool output 继续向第三方模型发起下一轮，直到得到最终 message 后再按 Responses SSE/JSON 返回给 Codex。不要把 hosted `web_search` 原样透传给第三方；第三方通常不会执行 OpenAI hosted tool，也可能因未知 tool type 报错。

## 2026-07-07 Hosted Tool Bridge Design Document

- 方案文档已落地到 `docs/codex-hosted-tool-bridge-design.md`。核心设计不是让第三方模型直接调用 OpenAI hosted tool，而是让 CCSwitchMulti 把 Codex 入站 Responses 请求里的 hosted tool（例如 `type=web_search`）替换为普通 function tool，第三方模型只发 function call，CCSwitchMulti 再用独立 OpenAI credential 调 OpenAI Responses hosted tool，并把结果规整为普通 tool output 回灌给第三方模型。
- 这个桥接模式可以扩展到 `image_generation`：对第三方模型暴露 `generate_image(prompt, size, quality, format)`，CCSwitchMulti 内部调用 OpenAI Responses `tools: [{ "type": "image_generation" }]`，解析 `image_generation_call.result`，把 base64 解码成 artifact 文件，返回路径、MIME、尺寸和可选缩略图；不要把完整 base64 直接塞回模型上下文或普通日志。
- `file_search` 也可桥接，但必须显式配置允许的 vector store、文件权限和日志脱敏；`computer_use` 不应作为普通 provider proxy tool-loop 的 MVP，因为它需要独立的交互会话、安全确认、屏幕状态和动作权限设计。
- 实现建议新增 `src-tauri/src/proxy/providers/hosted_tools/` 模块，拆出 `bridge.rs`、`openai_client.rs`、`web_search.rs`、`image_generation.rs`、`file_search.rs`。默认只开启 `web_search`，`image_generation` 和 `file_search` 需要显式 allowlist，日志只记录 trace id、tool name、query hash、耗时和状态，不记录完整网页正文、完整 prompt、base64 图片或 API key。

## 2026-07-10 新版 GPT/Codex 应用历史不可见与 standalone 修复器路径

- 现场根因不是历史数据丢失：当前 `~/.codex/config.toml` 的 live provider 是 `model_provider = "custom"`、模型是 `gpt-5.6-sol`，而 `~/.codex/state_5.sqlite` 中 1845 条 threads 里有 1839 条仍在旧 `codex_model_router_v2` 桶，只有 3 条在 `custom`。Codex 按当前 provider 桶过滤时会只显示极少新线程，看起来像“历史没了”。
- 同时存在两个 state DB：`~/.codex/state_5.sqlite` 今天仍在写入且有 1845 条；`~/.codex/sqlite/state_5.sqlite` 停在旧时间且只有 935 条。Rust/Tauri 内置修复器当前已经按 `sqlite_home` / `CODEX_SQLITE_HOME` / 根库 / `sqlite/` 旧库顺序选择 active DB，但 standalone Python 工具仍旧优先 `sqlite/`，会在离线修复流程里误修过期库。
- 已修 `scripts/codex-history-tool/codex_history_tool.py`：无显式 `--state-db` 时先尊重 `sqlite_home` / `CODEX_SQLITE_HOME`，再选当前根库 `~/.codex/state_5.sqlite`，最后兜底旧 `~/.codex/sqlite/state_5.sqlite`；`scripts/codex-history-tool/README.md` 同步说明该优先级。
- 验证基线：`python -m py_compile scripts\codex-history-tool\codex_history_tool.py` 通过；默认 dry-run 已命中 `activeDbKind=codex_root` 和 `stateDbPath=C:\Users\sunda\.codex\state_5.sqlite`，预览会把 1842 条旧 provider 行同步到当前 `custom` 桶，并发现 240 条可见候选；`cargo test --manifest-path src-tauri\Cargo.toml active_state_db --lib` 3 个 Rust active DB 选择测试通过。
- 操作边界：真正写入历史前必须让用户完全退出 Codex/GPT app 或使用内置修复器的并发保护；不要在 app-server 仍运行时强制写入 live SQLite。若目标是恢复 MultiRouter 稳定形态，还要另外排查为什么 live config 从 `codex_model_router_v2` 变成了 `custom`，这不是历史工具本身造成的。

## 2026-07-10 新版 GPT/Codex 保留 spawn_agent schema

- 用户截图中的错误 `Invalid Value: 'tools'. Function 'collaboration.spawn_agent' is reserved for use by this model and must match the configured schema.` 指向新版 GPT/Codex 后端对保留工具名的 schema 校验变严格；本机 live `~/.codex/config.toml` 当时存在 `[features.multi_agent_v2] hide_spawn_agent_metadata = false`，会让 Codex 给 `collaboration.spawn_agent` 追加 `model`、`reasoning_effort`、`service_tier` 等 metadata 字段，从而和新版模型的保留 schema 不一致。
- 旧修复思路是为了让 `spawn_agent` 参数里可直接覆盖模型；新版路径已经有 `~/.codex/agents/*.toml` custom agent role 文件承载子 Agent 的 `model`、`model_provider`、`model_reasoning_effort`，因此不应再通过扩展保留函数 schema 选择模型。
- `src-tauri/src/codex_config.rs` 的 Codex catalog/config 投影改为 `ensure_codex_multi_agent_reserved_schema_compatible`：保留用户原本 `multi_agent_v2` 启用状态，但写入 `hide_spawn_agent_metadata = true`，让 `collaboration.spawn_agent` 回到官方保留工具形态；文档 `docs/codex-spawn-agent-model-candidates.md` 同步说明子模型选择走 managed role 文件。
- 现场快速恢复可以把 `~/.codex/config.toml` 里的 `[features.multi_agent_v2] hide_spawn_agent_metadata = false` 改为 `true` 后重启 Codex/GPT app；长期修复依赖新版 CCSwitchMulti 重新投影配置。

## 2026-07-17 MultiRouter spawn_agent 保留 schema 二次根因

- v3.16.5-15 仍出现 `Function 'collaboration.spawn_agent' is reserved ... must match the configured schema` 时，`hide_spawn_agent_metadata=true` 已经正确，真正根因在模型目录合并：CCSM 用通用模板生成路由条目后覆盖了同 slug 官方模型的协议字段，把官方 `gpt-5.6-sol`、`gpt-5.6-terra`、`gpt-5.6-luna` 的 `use_responses_lite=true` 写成 `false`。现场 `models_cache.json` 与 `cc-switch-model-catalog.json` 对照可直接证明该差异。
- `use_responses_lite=false` 会让 Codex 不再使用 input 内的 `additional_tools` namespace transport，`collaboration.spawn_agent` 因而以不符合新版后端保留定义的工具结构发送；仅调整 `hide_spawn_agent_metadata` 无法修复这类 transport 级 schema 不匹配。
- `merge_codex_model_entry` 的正确所有权边界是：同 slug 官方缓存已经给出的协议、工具、推理和展示字段保持官方权威；CCSM 只覆盖模型标识、显式上下文、可见性、输入模态、用户明确声明的并行工具能力和基础指令等路由字段。回归测试 `codex_model_catalog_keeps_official_transport_and_reserved_tool_metadata` 必须覆盖 `use_responses_lite`、`multi_agent_version`、`tool_mode` 和 `apply_patch_tool_type`。
- 上述 transport 修复与 MultiRouter 空方案白屏修复一起发布为 v3.16.5-16；发布说明位于 `docs/release-notes/v3.16.5-16-zh.md`。

## 2026-07-10 统一 Codex App 历史目录与原生模型目录适配（纠正）

- 前一节“按 provider 桶过滤导致历史不可见”不适用于当前统一 Codex App，不能再作为新版历史修复根因。用 MSIX 内置 `codex 0.144.0-alpha.4` 按 App 官方同步参数只读实测，canonical `~/.codex/state_5.sqlite` 可稳定分页返回 `100 + 100 + 46 = 246` 条可见顶层 vscode 线程；`modelProviders` 省略、`null` 或空数组在当前数据上结果一致。两个数据库 `PRAGMA integrity_check` 都是 `ok`，历史正文和 canonical 索引没有丢失或损坏。
- 新版 `OpenAI.Codex_26.707.3748.0` 的桌面壳已经改为 `app/ChatGPT.exe`，子进程才是 `resources/codex.exe ... app-server`。新版侧边栏不直接展示 canonical `threads`，而是读取派生库 `~/.codex/sqlite/codex-dev.db` 的 `local_thread_catalog`。现场该目录只有 17 行，且 `local_thread_catalog_sync_state(local)` 为 `initial_build_complete=0`、`watermark_updated_at=NULL`，这才是“历史都没了”的真实断点。
- 新版主修复必须调用 App 自己的 `localThreadCatalog.requestStartupSync()`，由原生 manager 按 `useStateDbOnly=true`、source/parent/subagent/ephemeral 规则完成 cold sweep；不要批量重写 `state_5.sqlite` 的 `model_provider`、source/history 字段，不要改 rollout JSONL，也不要手工把 canonical rows 灌入 `local_thread_catalog`，否则会绕过 revision、missing-candidate、watermark 和 read-repair 语义。
- CCSM 的 renderer 兼容层现在通过新版 `rpc-*` 模块寻找 `localThreadCatalog`，调用 `readSnapshot()` / `requestStartupSync()`，并把“脚本已注入”和“原生同步确已请求”分开返回。Windows 宿主发现同时兼容旧 `Codex.exe` 与 `OpenAI.Codex_*` MSIX 内受路径约束的 `ChatGPT.exe`；普通独立 ChatGPT 程序不能进入 Codex CDP 链路。
- 旧版 SQLite/rollout 历史修复工具保留为离线兜底，但真实写入时只要检测到 Codex Desktop/app-server 仍运行就必须拒绝。它不是统一 App 的默认修复路径；dry-run 仍可作为只读诊断。
- `修复过程.zip` 的截图主要证明新版模型目录和路由适配，不是历史修复方案：官方 native cache 已含 `gpt-5.6-sol/terra/luna`，而 CCSM 路由 catalog 尚未包含。正确实现是按 `slug -> model -> id` 动态合并官方缓存与 CCSM 路由模型（路由字段优先、官方独有元数据与模型保留），并在当前缓存已被 CCSM 接管时从接管前 backup 恢复 native 基线；不要硬编码某一批 GPT-5.6 名称，也不要继续整份覆盖 `models_cache.json`。
- 验证基线：`cargo test codex_desktop::tests:: --lib` 17/17、`cargo test codex_history_migration::tests:: --lib` 39/39、`cargo test codex_config::tests:: --lib` 84/84、`pnpm typecheck`、`pnpm vitest run src/components/codex/CodexRouterWorkspacePage.test.ts` 39/39 均通过。

## 2026-07-11 Codex 内置 Image Gen 与 MultiRouter 边界

- 用户上传的“内置 Image Gen 调用失败”截图证明 Codex Desktop 内置图片生成不是走 `/v1/responses`，而是直接请求本地 provider base URL 下的 `POST /v1/images/generations`。因此 MultiRouter official `/responses` route 正常并不等于 Image Gen 会正常；如果本地代理没有注册 Images API 路由，请求会在 Axum 路由层直接 404，根本进不到 official route 解析。
- 修复入口是 `src-tauri/src/proxy/server.rs` 和 `src-tauri/src/proxy/handlers.rs`：FullProxy 与 External OpenAI API 模式都要注册 `/v1/images/generations`。普通 OpenAI-compatible provider 应按 Images API 原样透传；当前 provider 是 official OAuth 或 MultiRouter 能物化出 official OAuth route 时，图片请求优先走 ChatGPT/Codex 官方 OAuth 目标。
- 图片请求的 `model` 往往是 `gpt-image-*`，旧 MultiRouter official route 可能只匹配 `gpt-5.x` 文本模型。探测 official route 时可以用真实图片模型、catalog 里的 `gpt-*` 模型和稳定 GPT/Codex 名称做只读 probe，但发送 Images API 时必须保留请求体里的 `gpt-image-*`，并移除 request-local route 上的 `codexResolvedUpstreamModelOverride`，否则会把图片模型误写成 `gpt-5.5`。
- text-only 能力仍要优先于模板/模型配置的 `input_modalities`：`src-tauri/src/codex_config.rs::codex_catalog_model_entry` 在 NativeResponses 分支不能把已判定 text-only 的模型重新写回 `["text","image"]`，否则 Codex Desktop 会误显示内置 Image Gen 或多模态入口。
- 回归覆盖：`cargo test --manifest-path src-tauri\Cargo.toml image_generation --lib` 覆盖 Images 路由进入 handler 与 MultiRouter official route 选择；`cargo test --manifest-path src-tauri\Cargo.toml codex_model_catalog_text_only_native_responses_never_reenables_image_modality --lib` 覆盖 text-only 不被 NativeResponses 模态覆盖。

## 2026-07-13 Codex 多设备额度协作

- 官方 `utilization` 是账号总窗口百分比，本地 token 只是单设备代理/会话日志口径，禁止相互换算；协作页必须将“账号总窗口”和“已接入设备 token”拆开呈现。
- 协作账号命名空间由 `~/.codex/auth.json` 的 `tokens.account_id` 加固定域分隔后 SHA-256 得到；真实账号 ID、OAuth token、prompt、原始 JSONL、cwd 都不写入远端或本地协作报告。
- 远端协议：`{remote_root}/quota-collaboration/v1/{account_scope}/{device_id}.json`。每台设备只覆盖自己的文件；WebDAV 通过 `PROPFIND Depth=1`、S3 通过 `ListObjectsV2` 发现同 scope 下其它设备，避免共享 manifest 的多机覆盖竞态。
- `quota_collaboration_reports` 只保存每设备最新脱敏聚合报告。刷新官方 Codex 额度时写入本机报告；“同步设备报告”上传本机文件并合并其它设备。损坏、scope 不匹配或百分比越界的报告必须跳过。
- `observe` 只看数据。`enforce` 使用最近 10 分钟内、所有已同步设备中每个官方窗口的最高 utilization；窗口剩余不高于阈值时，`RequestForwarder` 只拒绝经过本机 CCSwitchMulti 的 Codex 请求（HTTP 429）。未通过 CCSwitchMulti 的旁路请求无法被控制，任何 UI 或文档不得承诺全账号强制控制。
- 用户文档位于 `docs/guides/codex-multi-device-quota-collaboration-zh.md`。首次接入必须在每台设备刷新官方额度、设置独立设备名并同步；不要复制 settings/配置目录，否则相同 `deviceId` 会使设备报告互相覆盖。
- 验证覆盖：`quota_collaboration` 有 29 项 Rust 测试，包含内存 DB 的设备持久化/隔离、observe 和 stale 不拦截、最高官方 utilization 触发约束，以及本地 mock WebDAV 的 MKCOL/PUT/PROPFIND/GET 两设备发现路径；前端 `CodexUsagePage.test.tsx` 覆盖未配置引导、约束确认和设置保存 payload。不要让测试读取真实 `auth.json` 或写入真实 settings。

## 2026-07-14 Issue #12 商汤 Chat Provider 协议误判根修

- 真实根因：`explain_codex_responses_upstream_protocol` 的已知 Chat-only URL 列表缺少 `sensenova.cn`，导致 `token.sensenova.cn/v1` 在协议信号缺失时退回 Native Responses；MultiRouter 物化目标 Provider 时也会丢失 route 的 `apiFormat`，并可能被目标 Provider 的陈旧 meta 再次覆盖。
- 修复：`materialize_codex_routed_provider_from_target` 同步 route 的 `apiFormat` / `api_format`，并让 route 的显式协议覆盖目标 Provider 的陈旧 `meta.apiFormat`；`is_known_chat_completions_only_url` 增加 `sensenova.cn` 作为旧配置兜底。
- 回归测试覆盖 SenseNova URL 推断，以及“目标 meta 为 `openai_responses`、route 显式为 `openai_chat`”的冲突场景。协议修正后继续复用现有 Responses -> Chat 工具调用映射，不做厂商专用 tool call ID 改写。

## 2026-07-14 Issue #15 同会话切换模型仍落回官方额度

- MultiRouter 不按 session 固定 route；每次请求都读取当前 `body.model`。`v3.16.5-5` 已收紧 catalog，只注入启用 route 的模型，但 Codex Desktop 仍可能暂存旧 alias。
- 旧 alias 若只属于已停用 route，原 resolver 会把它当成普通未匹配模型并使用 `defaultRouteId=official`，从而把“中转模型已失效”表现成“官方额度耗尽”。
- 根修是在 enabled route 无匹配时检查 disabled route 的精确模型声明；命中旧 alias 就 fail closed，不再静默回官方。回归同时覆盖同一 router 连续从 official 模型切到 relay alias 时，第二个请求按新 `body.model` 重新解析到 relay。

## 2026-07-14 CCSwitchMulti v3.16.5-8 发布准备

- fork 最新正式 Release 是 `v3.16.5-7`；本地 `v3.17.0` tag 来自 upstream，fork 不存在该 tag，因此本次继续使用 fork 补丁版本 `v3.16.5-8`。
- 发布范围以 `v3.16.5-7..main` 的真实 diff 为准：多设备额度协作、可信 Codex originator 白名单保留与缺失回退、SenseNova Chat 协议根修、停用 route 旧 alias 禁止回官方，以及相关教程和回归测试。
- `.github/workflows/release.yml` 由 `v*` tag 触发，必须先让 main CI 的 Prettier/rustfmt 门槛通过，再推 annotated tag 到 `fork`；完成后核验 Release 为正式版、矩阵任务成功、跨平台资产和 `latest.json` 齐全。
- CI 后端测试曾在 UTC runner 上失败：`range_starts_midnight_boundary` 用固定 `UTC+8` 构造样本，但被测函数按运行机器 `Local` 时区分日，导致同一 `UTC+8` 日期在 UTC 下跨日。测试已改为用 `Local` 构造边界样本，保持生产统计按设备本地日历日计算的语义。
- `v3.16.5-8` 已于 2026-07-14 发布到 `BigStrongSun/ccswitchmulti`：tag 解引用提交为 `27bdcdfa5e3733c0f9cd3fa37bb5192606ba8e23`，Release workflow run `29276721016` 成功，Release 为 `draft=false`、`prerelease=false`。
- Release 共 19 个资产，覆盖 Windows x64/ARM64 setup 与 portable、macOS dmg/tar.gz/zip、Linux x64/ARM64 AppImage/deb/rpm、5 个 updater 签名文件和 `latest.json`。
- `latest.json` 验证为 `version=3.16.5-8`，平台键包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64`，每个平台都有下载 URL 和 signature。

## 2026-07-14 Codex Multi-Agent Agent Message Content Boundary

- CCSwitchMulti `v3.16.5-7` / `v3.16.5-8` 在 official OpenAI OAuth `/responses` 路径可能返回 `Missing required parameter: 'input[20].content'`。该错误不依赖本机日志即可从请求归一化链路复现：`normalize_codex_oauth_input_item` 过去只允许 `type=message` 保留 `content`，其余未知类型一律删除。
- OpenAI Codex 在 2026-06-12 起把 Multi-Agent V2 投递表示为 `type=agent_message`，字段包括必填的 `author`、`recipient`、`content`；`content` 可同时包含 `input_text` 和 `encrypted_content`。`gpt-5.6-sol` 使用多 Agent 后，历史中的第 21 条恰好可能是这种 item，旧 normalizer 会把合法请求改坏后再转发。
- 根修复不是给缺失字段补空数组，也不是停止清理所有 tool content。official OAuth normalizer 改为明确的“禁止 content 类型列表”：function/custom/MCP/tool-search/local-shell/web-search/image-generation/compaction 等调用或输出类型继续删除冗余 `content`；`reasoning` 继续转换到 `summary`；`message`、`agent_message` 和未来未知类型保守保留原始 `content`。
- 回归测试 `codex_oauth_responses_normalizer_preserves_agent_message_content` 构造 21 条历史并断言 `input[20]` 的明文和加密 content 均保留；`codex_oauth_responses_normalizer_preserves_unknown_item_content` 固定未知类型的前向兼容回退。原有 tool output 和 reasoning content 清理测试继续约束私有 backend 的严格边界。

## 2026-07-14 Codex OAuth Content Cleanup Source And Responses-Lite Boundary

- `agent_message.content` 回归的源头提交是 CCSwitchMulti `77781164`（2026-07-01）。该提交为修复 `function_call_output`、`custom_tool_call_output`、`tool_search_output` 的 `content array too long`，把已证实的三个禁止类型泛化成“仅 message/reasoning 允许 content”；测试也只覆盖这三个输出类型，没有未知类型默认行为。`cb337248`（2026-07-03）正确把 reasoning raw content 改为 summary，但把 allow-list 进一步收窄到仅 message，使后续新增的 agent_message 必然被误删。上游 Codex 的对应演进是 `8f2d6416`（2026-06-12）把 plaintext/encrypted Multi-Agent V2 投递统一为 typed `agent_message`，`5b22a8e5`（2026-06-16）固定 NEW_TASK/MESSAGE/FINAL_ANSWER envelope；因此 CCSwitchMulti 7 月 1 日引入 allow-list 时其实已经落后于上游 schema。
- 同一审计发现 Responses-Lite 也被旧 normalizer 改坏：上游 Codex `33cc928d`（2026-06-23）开始让 Lite 请求用 `input[0].type=additional_tools` 携带 tools，并用 developer message item 携带 instructions，顶层 `tools/instructions` 应保持缺失。旧路径无视 Lite header，仍提升 developer message 并补顶层空 tools，破坏官方请求形态。
- Lite fallback 旧实现只剥 `x-openai-internal-codex-responses-lite` header 后重发原 body；这会形成“标准 header + Lite body”的协议错配。修复后，正常 Lite 转发只做 function arguments 和 OAuth item content 的安全清理，不移动 input instructions/tools；命中 fallback 缓存或上游明确拒绝 Lite 时，同时提取 `additional_tools` 到顶层 tools、提升 developer/system message 到 instructions，再用标准 Responses body 重试。
- content 规则增加参数化类型矩阵，覆盖 additional_tools/item_reference/function/custom/MCP/tool-search/local-shell/computer/file-search/code-interpreter/web-search/image-generation/compaction 等已知非 content item；message、agent_message 和未知类型继续保守保留。以后新增 item 类型时必须显式选择“已知禁止”或“未知保留”，并同步测试，不能恢复 catch-all allow-list。

## 2026-07-14 Codex 新 Session 模型菜单空列表竞态

- 官方 app-server `model/list` 响应固定为 `{ data, nextCursor }`，并允许合法返回 `data=[]`。Desktop 兼容脚本旧实现只对非空 `data` 调用 `patchModelArray`；首次空响应因此原样进入 renderer 查询缓存，新建 session 会表现为模型选择入口缺失或一直未加载。
- renderer 修复必须同时覆盖 `list-models-for-host` 和原生 `model/list` 两种方法名，并只处理已记录 request id 的 `mcp-response`。对已确认的模型列表响应，`data=[]`、`models=[]` 和直接数组都允许从 CCSM payload 回填；不能继续用通用 object graph patch 猜测其它 MCP 响应。
- Desktop 解锁目录投影不能只读取一次 `cc-switch-model-catalog.json` 后失败即安装空 payload。读取顺序改为生成 catalog、带 `etag=cc-switch-model-catalog` 的当前 models cache、活动 `model_provider` 的内联 `models`；回退仍严格限定活动路由目录，不能重新引入未启用 provider。
- 修改 renderer 兼容脚本行为时必须同步升级 patch key 和 request-client patch 版本；否则已运行 renderer 会因旧 installed 标记跳过新版拦截器，导致“代码已更新但重新解锁仍无效”。
- 官方 Codex 当前 `model/list` 使用 `RefreshStrategy::OnlineIfUncached`，标准响应类型见 `ModelListResponse { data, next_cursor }`。CCSM 的兼容层应把空列表视为可恢复的目录初始化竞态，而不是永久有效的“没有模型”结果。

## 2026-07-14 CCSwitchMulti v3.16.5-9 发布结果

- `v3.16.5-9` 已发布到 `BigStrongSun/ccswitchmulti`，tag 解引用的发布构建源码提交为 `c26237a1ad09cd6c21b7d3fd899786d99014b7f8`；正式 Release 为 `draft=false`、`prerelease=false`。
- 发布前 main CI run `29307562657` 成功，覆盖前端 typecheck、format、unit tests，以及后端 rustfmt、Clippy 和完整 Rust tests。
- Release workflow run `29308202757` 成功，Windows x64、Windows ARM64、Linux x64、Linux ARM64、macOS 五个构建矩阵，以及 Publish GitHub Release、Assemble latest.json 全部通过。
- Release 共 19 个资产，包含 Windows x64/ARM64 setup 与 portable、macOS dmg/tar.gz/zip、Linux x64/ARM64 AppImage/deb/rpm、5 个 updater 签名文件和 `latest.json`。
- `latest.json` 验证为 `version=3.16.5-9`，平台键包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64`，每个平台都有下载 URL 和 signature。

## 2026-07-15 CCSwitchMulti v3.16.5-10 白屏与数据库兼容发布结果

- `v3.16.5-10` 已发布到 `BigStrongSun/ccswitchmulti`。annotated tag 解引用到发布源码提交 `212c512f0962e6e12e7ccc9ef2e16d8937eab9e3`；正式 Release 为 `draft=false`、`prerelease=false`，地址为 `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.5-10`。
- 发布内容同时覆盖两条白屏故障链：多路路由向导对历史异常模型目录项的未捕获渲染异常，以及应用根树的未捕获渲染异常恢复页；另将 SQLite schema 从 v12 迁移至 v13，并为旧数据库补齐 `input_token_semantics` 字段，兼容上游 v3.17.0 已写入 v13 的使用统计库。
- Release workflow run `29354860819` 成功，Windows x64、Windows ARM64、Linux x64、Linux ARM64、macOS 五个构建矩阵，以及 Publish GitHub Release、Assemble latest.json 全部通过。Release 共 19 个资产；`latest.json` 已验证为 `version=3.16.5-10`，六个平台键均有下载 URL 和 signature。

## 2026-07-15 MultiRouter 同 Session 模型上下文窗口切换根修

- 用户报告的“短上下文模型切到长上下文模型仍是短窗口，反向切换又可能向短模型注入超长历史”根因是 CCSwitchMulti 旧提交 `3748fe8a7be113b01965c7e46cbe3e6b9688dfd6` 把顶层 `model_context_window` 当作跨 provider 的用户设置保留。对单模型 provider 合理，但 MultiRouter 将多个模型放入同一 `codex_model_router_v2` provider 后，该顶层值会覆盖 catalog 中所有模型的 `context_window`。
- 官方 Codex 当前 `models-manager/src/model_info.rs::with_config_overrides` 会把顶层 `model_context_window` 和 `model_auto_compact_token_limit` 无条件覆写每个 `ModelInfo`。官方 `core/src/session/turn.rs::maybe_run_previous_model_inline_compact` 则每个 turn 使用当前 `ModelInfo` 的窗口，并在长窗口切短窗口且历史已超过新窗口时，先使用旧模型执行 `ModelDownshift` 压缩，再运行短模型。不要把全局窗口写成最大值，这会再次破坏下行切换保护。
- CCSwitchMulti 修复位于 `src-tauri/src/codex_config.rs`：启用 `codexRouting` 或实际 config 已指向 `codex_model_router_v2` 时，目录投影会移除顶层 `model_context_window` 和 `model_auto_compact_token_limit`；live config 合并也会再次移除它们，避免旧配置在首次接管、热切换或恢复时回灌。普通单模型 provider 不受影响，仍保留用户的显式全局覆盖。
- 验证：`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、两条定向回归、`cargo check --manifest-path src-tauri/Cargo.toml --lib` 和 `cargo test --manifest-path src-tauri/Cargo.toml codex_config::tests:: --lib`（89/89）均通过。回归固定 MultiRouter 删除全局窗口/固定压缩阈值且保留逐模型目录，并固定单模型 provider 保留用户覆盖。

## 2026-07-15 CCSwitchMulti v3.16.5-11 发布结果

- `v3.16.5-11` 已发布到 `BigStrongSun/ccswitchmulti`。annotated tag 解引用到发布源码提交 `667de3dc3c9ecf1fe355a34f56d3dd8296d87102`；正式 Release 为 `draft=false`、`prerelease=false`，地址为 `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.5-11`。
- 发布内容为 MultiRouter 同一 Codex session 的按当前模型上下文窗口切换根修：短模型切长模型恢复目标模型窗口，长模型切短模型维持官方发送前压缩保护；单模型 provider 的全局覆盖保持不变。
- Release workflow run `29362697306` 成功，Windows x64、Windows ARM64、Linux x64、Linux ARM64、macOS 五个构建矩阵，以及 Publish GitHub Release、Assemble latest.json 全部通过。Release 共 19 个资产；`latest.json` 已验证为 `version=3.16.5-11`，六个平台键均有下载 URL 和 signature。

## 2026-07-15 Provider settingsConfig 为 null 的启动崩溃根修

- 截图中的错误 `Cannot read properties of null (reading 'settingsConfig')` 不是 v3.16.5-10 已处理的 MultiRouter 目录渲染异常，而是另一条 Provider 运行时契约破坏链。仅凭这条 JavaScript 文案不能区分“provider 条目本身为 null”和“上游先返回了 settingsConfig=null、随后某处派生出空 provider”；前端 `Provider` 类型声明不会校验真实 IPC payload，因此两层边界都必须收紧，不能只在两个报错组件旁加空值判断。
- 根修在 `src-tauri/src/database/dao/providers.rs`：所有 provider 配置解析、列表读取、按 ID 读取、OMO 当前 provider 读取、保存和局部更新都统一规范化为 JSON object。`null`、数组、标量、空/损坏 JSON 一律成为 `{}`，有效对象不变；因此旧库无需手改，也不会再被新写入重新污染。
- 回归：DAO parser 覆盖 `null`、数组、标量、损坏文本和有效对象；内存 SQLite 覆盖列表读取、按 ID 读取及写回 `null` 后实际持久化为 `{}`。`cargo fmt --check`、`cargo test --manifest-path src-tauri/Cargo.toml database::dao::providers::tests --lib`（2/2）和 `cargo check --manifest-path src-tauri/Cargo.toml --lib` 通过。本机 `~/.cc-switch/cc-switch.db` 只读扫描未发现当前坏记录，说明需修的是跨用户历史数据入口而非本机现存库。

## 2026-07-15 GitHub ARM64 Release 依赖安装网络容错

- `v3.16.5-13` 的 Release workflow 连续两次只有 Linux ARM64 失败；两次日志都显示 `ports.ubuntu.com` 的 IPv6 路由不可达，随后 IPv4 端口 80 超时。Windows x64/ARM64、macOS 与 Linux x64 均成功，因此失败与白屏源码或 Rust/前端构建无关。
- 旧工作流的 Linux 系统依赖阶段对每条 `apt-get` 只尝试一次，任一 Ubuntu 软件源瞬断就会让发布步骤整体跳过。根修是在统一 `apt_get` helper 中强制 IPv4、启用 APT 下载重试和连接超时，并对完整事务做三次退避重试；`update`、核心依赖、GTK、WebKit 与 libsoup fallback 全部必须经过该 helper。
- 已推送且失败的发布 tag 不应改写。`v3.16.5-13` 保留为失败构建证据，包含工作流根修的新源码应使用后续补丁版本重新打 tag。

## 2026-07-16 CCSwitchMulti v3.16.5-14 白屏根修发布结果

- 用户再次提供 `Cannot read properties of null (reading 'settingsConfig')` 恢复页截图时，`v3.16.5-13` 尚未生成 GitHub Release：其两次 Release attempt 均因 Linux ARM64 无法连接 `ports.ubuntu.com` 而失败，Publish Release 被跳过。因此该截图只能证明旧安装包仍会触发问题，不能证明 `cfb4804d` 的 Provider IPC 隔离已经在用户机器运行。
- `v3.16.5-14` 已发布到 `BigStrongSun/ccswitchmulti`，annotated tag 解引用与远端 main 均为 `13e16af9f12299161f1bd2ce5d86a5f0438235a0`。正式 Release 为 `draft=false`、`prerelease=false`：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.5-14`。
- 主 CI run `29430225565` 成功；Release workflow run `29430899551` 的 Windows x64、Windows ARM64、Linux x64、Linux ARM64、macOS、Publish GitHub Release 与 Assemble latest.json 全部成功。Linux ARM64 在强制 IPv4和重试后越过依赖安装，验证 `4a6858a3` 的发布网络容错有效。
- Release 共 19 个资产。`latest.json` 验证为 `version=3.16.5-14`，包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64`，每个平台都有非空下载 URL 和 signature。
- 发布快照的干净工作树验证通过：Provider payload 与 AppErrorBoundary 共 4 项测试、TypeScript typecheck、renderer 生产构建、app_exit_monitor 2 项测试及 `cargo check --lib`。本地未提交的 `/responses/compact` 代理改动未进入发布提交或 tag。

## 2026-07-16 MultiRouter 手动 Chat 协议被旧 Provider 快照回滚

- 用户在“配置核心参数”里把 Qwen 明确选成 Chat Completions，但“生成路由规则”仍显示 `openai_responses`；有时连续点击几次又会成功。这种非确定性不是协议探测结果本身，而是父层 provider query refetch 与向导草稿写入之间的竞态。
- `CodexMultiRouterWizard` 打开后的同步 effect 原本用 `nextSourceById.get(id) ?? currentById.get(id)` 合并 provider，导致任何父层 refetch 都优先采用数据库旧快照，覆盖用户刚写入草稿的 `apiFormat/apiFormatSource=manual`，也可能覆盖刚刷新的模型目录或别名处理结果。
- 根修边界：向导打开期间，已选 source 以 `currentById` 草稿为事实来源，父层快照只补充草稿中尚不存在的 source，并继续同步删除项；从 Provider 配置页返回时向导会关闭再打开，由初始化 effect 读取最新数据库配置，不需要在打开期间破坏草稿。

## 2026-07-15 v3.16.5-11 远端白屏报告复盘与前端 IPC 防线

- 用户回传的 `cc-switch.log` 显示 v3.16.5-11 后端完成数据库、provider、代理和 Codex 历史初始化；`codex-router.log` 中大多数请求正常得到上游 200。React/WebView 的 `Cannot read properties of null (reading 'settingsConfig')` 不会自动写入 Rust 日志，因此“后端日志无报错”不能推翻前端白屏报告，也不能据此归因网络或本机配置。
- `app-exit-events.jsonl` 中 2026-07-15 11:18:55 的 `cannot move state from Destroyed` 是 tao 0.34.6 Windows 事件循环 panic；随后 11:19:58 重启并可正常主动退出。这是独立的窗口生命周期异常，不是 `settingsConfig` React 空引用的证据，后续应另案复现，不能混入本次 Provider 数据契约修复。
- `7b7d9679` 已提交且随 v3.16.5-12 发布，负责数据库 DAO 的 `settings_config` 对象化；此前工作区中另有未提交的前端防线。正式收口应放在 `providersApi.getAll()` 统一 IPC 边界，而不是只放首页 `useProvidersQuery`：根 payload 非对象、provider 条目为 null/缺 id/缺 name 时隔离，`settingsConfig` 非对象时归一 `{}`，从而覆盖 query、mutation、设置页等所有直接调用者。
- `AppErrorBoundary` 继续作为最后一道恢复与取证边界：未捕获渲染异常显示恢复页，并把错误消息、组件栈和发生时间保存到带版本的本地诊断键 `ccswitchmulti.lastRenderError.v1`。它不替代数据边界修复，也不捕获异步事件或原生窗口 panic。

## 2026-07-15 CCSwitchMulti v3.16.5-12 发布结果

- `v3.16.5-12` 已发布到 `BigStrongSun/ccswitchmulti`。annotated tag 解引用到发布源码提交 `063a6d59bf74893e021efd3fe75045b47e14baa2`；正式 Release 为 `draft=false`、`prerelease=false`，地址为 `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.5-12`。
- 本版发布 Provider `settingsConfig` 非对象配置兼容根修，消除截图中的应用启动错误边界；用户无需手工编辑 SQLite，旧记录会在读取时恢复为安全空对象。
- Release workflow run `29384516268` 成功，Windows x64、Windows ARM64、Linux x64、Linux ARM64、macOS 五个构建矩阵，以及 Publish GitHub Release、Assemble latest.json 全部通过。Release 共 19 个资产；`latest.json` 已验证为 `version=3.16.5-12`，六个平台键均有下载 URL 和 signature。

## 2026-07-15 GPT-5.6 偶发 `collaboration.spawn_agent` HTTP 400 复核

- `gpt-5.6-sol` 和 `gpt-5.6-terra` 都返回 `Function 'collaboration.spawn_agent' is reserved for use by this model and must match the configured schema`，证明失败发生在上游校验 Responses `tools` 数组时，尚未进入模型推理；不是 Sol/Terra 子型稳定性差异，切换 5.6 子型不是根修。
- 所谓“有时好用”只能说明不同请求构造了不同的工具集，或 Codex app-server 进程/会话仍持有旧配置；一旦同一请求携带了被扩展的保留 schema，上游会在推理前确定性返回 400，不是随机网络错误。
- 截图分析里的 `spawn_agent_visible_model_limit` 只控制候选模型展示窗口，`src/services/quota_collaboration.rs` 只处理额度协作，都不参与 `collaboration.spawn_agent` 的函数 schema 生成，不应作为本错误的根因线索。
- 本机当时安装 CCSwitchMulti `3.16.5-12`，active `~/.codex/config.toml` 已是 `[features.multi_agent_v2] hide_spawn_agent_metadata = true`，当前 Codex app-server 进程也晚于该配置启动；本机近期 session 未找到新的同类上游错误。对外部报错机器应直接核对其 active config 和 app-server 启动时间，不能用本机健康状态代替现场证据。

## 2026-07-15 Codex 历史迁移后 Desktop 全局卡顿根因

- CCSM 旧的 provider bucket 迁移会在 Codex Desktop/app-server 运行时批量替换 rollout JSONL，并让文件获得新的 mtime；这会同时使 Codex 原生历史缓存失效、触发 SQLite/WAL 竞争，并让 CCSM 的使用量同步器把旧历史当作活跃文件重新扫描。根修是在任何真实写入前检测 Codex 进程并返回 `codex_running`，只保留 dry-run 诊断能力。
- provider bucket 改写只改变元数据，不代表会话最近活跃。原子替换 JSONL 后必须恢复原始 mtime，避免 Codex Desktop 和 CCSM 周期同步器重新索引全部迁移历史。
- 当前 `~/.codex/config.toml` 已由 Codex CLI 成功解析，活动 provider 为内置 `openai`，没有 CCSM 本地 URL、`PROXY_MANAGED` 或自定义 catalog 残留；本次现场不能归因为当前 TOML 损坏。
- 当前 Codex 现场约 17 个已加载任务产生 16 组 MCP/plugin runtime，约 84 个 MCP 后代进程、约 3 GB RSS。失效远程 Docs MCP 是单点启动延迟，但任务级 MCP 重复实例化是当前资源放大的主因；这是 Codex runtime 生命周期问题，CCSM 不应通过改写历史制造更多“最近活跃”任务来进一步放大。
- CCSM 仍需后续处理：Codex 使用量同步器对每个已修改 rollout 从首行重放；代理接管会临时删除整个 `[model_providers]` 后只创建 CCSM provider，恢复过度依赖备份；代理只覆盖模型 API，MCP、OAuth、插件和 Desktop 服务仍直连，UI 与文档必须明确该边界。

## 2026-07-16 CCSwitchMulti v3.16.5-15 MultiRouter 协议竞态修复发布结果

- `v3.16.5-15` 已发布到 `BigStrongSun/ccswitchmulti`。annotated tag 的本地与远端解引用均为发布源码提交 `c800c546ae662d3d01004c6532c75c95b267f59b`；正式 Release 为 `draft=false`、`prerelease=false`，地址为 `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.5-15`。
- 本版修复 MultiRouter 向导中手动选择 Chat Completions 后偶尔又显示 Responses 的竞态：向导打开期间以当前草稿为事实来源，父层 Provider query refetch 只补充尚未进入草稿的 source，不再用旧数据库快照覆盖用户刚选的 `apiFormat/apiFormatSource=manual`。
- 发布前独立干净快照通过定向 UI/lib 回归 52/52、TypeScript typecheck、renderer 生产构建、Rust `cargo check --lib`、Prettier、rustfmt 与 `git diff --check`。main CI run `29434619832` 的 Frontend Checks、Backend Checks、Clippy 和完整测试全部成功。
- Release workflow run `29435226376` 成功；Windows x64、Windows ARM64、Linux x64、Linux ARM64、macOS 构建矩阵，以及 Publish GitHub Release、Assemble latest.json 全部通过。macOS DMG 的公证与代码签名校验成功。
- Release 共 19 个资产。`latest.json` 验证为 `version=3.16.5-15`，包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64`，每个平台都有非空下载 URL 和 signature。
- 本地未提交的 `/responses/compact` 代理改动及旧输出目录没有进入发布提交、tag 或本次发布结果记录提交。

## 2026-07-16 第三方 Agent API 的 MultiRouter 聚合来源可用性根修

- 第三方 Agent API 已支持把整个 Codex MultiRouter 选为聚合模型源，也保留单条 route 的高级直连入口；不需要再增加一套独立的“多 Provider 多选”配置，否则会与 MultiRouter 的匹配、默认路由和故障转移规则形成两个事实来源。
- 截图中 Router 与其所有 route 都显示“需补配置”的根因是 External API 预检只读取 route 内联凭据或 Router 外壳凭据，而新版 route 只保存 `targetProviderId`，真实 Base URL、API key、协议及 OAuth 状态位于被引用的目标 Provider。实际请求会物化目标 Provider，因此旧预检与运行时语义不一致。
- 根修后，预检按运行时相同的字段优先级解析新旧 route schema，并从同一应用的 Provider map 查找目标；目标存在时优先按目标 Provider 判断托管 OAuth 或 Codex adapter 可提取的真实 Base URL/认证。显式引用缺失目标时必须不可用，不能被 route 上残留的 managed OAuth 标记误判为可用；没有目标引用的旧 route 继续兼容内联凭据和 Router 外壳凭据。
- Codex Provider 的运行时能力不能用只服务于额度查询的 `resolve_usage_credentials()` 代替：后者主要解析 config TOML，真实 Codex adapter 还支持顶层 `base_url/baseURL`、auth/env/direct key。第三方 API 的可用性检查已改为复用 adapter 提取规则，同时将托管 OAuth 与静态凭据视为并列认证路径，空配置的官方 seed 仍保持可用。
- 回归覆盖：整个 Router 可聚合官方 OAuth 与 `targetProviderId` 指向的 Qwen 静态 Provider；两条 route 均可用并贡献模型；缺失目标 Provider 时 Router 和 route 均不可用且返回可诊断错误。验证通过 External API 16/16、Codex 目标物化 6/6、外部 Router 解析 2/2、TypeScript typecheck、Prettier、rustfmt 和 `cargo check --lib`。

## 2026-07-16 CCSM 重启后 Codex 托管 OAuth 账号消失根修

- 用户反馈从 v3.16.5-13 连续升级并频繁重启 CCSM/Codex 后，连接测试可能先报 401，随后认证中心直接显示未认证。`v3.16.5-13..v3.16.5-15` 在 `codex_oauth_auth.rs`、统一 auth commands、Codex OAuth commands 和 manager 初始化处没有差异，因此不是 -14/-15 升级脚本新引入的认证库迁移或清理。
- 当前 `CodexOAuthManager` 只把 access token 放在内存，重启后的新 manager 必然没有 access token；第一次模型目录、额度或真实代理请求会调用 `get_valid_token_for_account()` 并立即使用磁盘 refresh token。连接测试/页面自动模型查询因此可以成为重启后的首次刷新触发器。
- `refresh_with_token()` 当前把 token 端点任意 HTTP 401/403 都折叠成 `RefreshTokenInvalid`，不检查 OAuth error body；调用方随后立即执行 `remove_invalid_account_after_refresh_failure()` -> `remove_account()` -> `save_to_disk()`，把账号从 `codex_oauth_auth.json` 永久删除。认证页的离线状态只看本地账号集合，所以删除后直接显示未认证；这解释了“先 401，随后认证页账号消失”，不是单纯 UI 状态错误。
- 本机日志也显示每次进程启动后均出现“从磁盘加载 1 个账号”后紧跟“access_token 需要刷新”，且同一启动时刻可能有多个调用同时到达刷新入口；每账号锁能合并网络刷新，但不能消除重启必刷。现场是否为 refresh token 真正轮换/撤销、并行旧进程竞争，还是 token 端点/WAF/代理的暂时 401/403，需要用户机器的 `cc-switch.log` 中 `[CodexOAuth]` 邻近日志确认；不要索取或导出含 refresh token 的 `codex_oauth_auth.json`。
- 根修不能只延迟刷新或给 UI 加提示：`refresh_with_token()` 现只按结构化 OAuth error code 识别 `invalid_grant`、`invalid_refresh_token`、`refresh_token_invalid`；普通 401/403 作为可重试失败保留账号。错误只提取经过字符约束的结构化 code，原始 body 不进入日志/UI，避免上游意外回显敏感内容。
- 明确失效的账号不再物理删除，而是在 `codex_oauth_auth.json` 中持久化 `invalidated_at/auth_error` 隔离；默认账号、账号列表和代理只选择可用账号，认证页显示“记录已保留，请重新登录”，同账号重新授权会原位覆盖并清除失效态，只有用户显式移除/注销才删除记录。
- 短期 access token 与过期时间现在和 refresh token 保存在同一凭据文件中，进程重启优先恢复未临期 token，不再每次启动都强制刷新。刷新成功会把 access token、过期时间及轮换后的 refresh token 作为同一串行快照落盘。
- refresh 返回明确 invalid_grant 后会在隔离账号前再次加载磁盘；若另一个进程刚写入不同 refresh token 或仍有效 access token，则采用最新凭据再试一次。这样旧进程的飞行中请求不能用旧 token 的失败覆盖新进程刚完成的轮换。
- 验证覆盖 24 项 Codex OAuth 定向回归，包括重启复用短期 token、临时 401 保留账号、明确 invalid_grant 隔离但不删除、同账号重新登录原位恢复、跨 manager 读取轮换 token、飞行中跨进程轮换恢复；TypeScript typecheck、`cargo check --lib`、Clippy `-D warnings`、rustfmt 与 Prettier 通过。

## 2026-07-17 v3.16.5-15 保留工具报错与 MultiRouter 白屏重新定界

- 用户明确故障机器运行 `3.16.5-15`。截图中的 macOS 资源名 `index-D-I7smwQ.js` 已在 GitHub Release `v3.16.5-15` 的正式 `CCSwitchMulti-v3.16.5-15-macOS.zip` 二进制中命中，`Info.plist` 也为 `3.16.5-15`；因此不能再把两张截图归因为旧安装包、错误版本号或未升级。
- `null is not an object (evaluating 'e.settingsConfig')` 的堆栈落在该发布包的 Codex MultiRouter 工作台主 chunk。`v3.16.5-15` 虽然在数据库 DAO 和 `providersApi.getAll()` 的 `get_providers` IPC 边界规范化了 provider，但工作台仍有 React Query 缓存、乐观更新、组件 props 和派生集合等内存入口直接信任 `Provider` 非空。之前“数据库与单一 IPC 双边界已经完整封闭白屏”的结论过宽；后续根修必须给共享 query/cache 写入和工作台组件入口建立同一运行时规范化契约，并用 null provider 注入回归覆盖真实工作台渲染链。
- `collaboration.spawn_agent` 保留 schema 报错也不能只凭 `hide_spawn_agent_metadata=true` 判定已修。当前 Codex 工具规划还受 multi-agent 版本、tool namespace、动态 feature config 和 Codex 运行时版本共同影响；新源码已出现 `multi_agent_v1` namespace，而截图现场仍是 `collaboration.spawn_agent`。必须从故障机采集报错请求的实际 `tools`/namespace/schema 与 Codex 版本，再与同版本官方直连请求做结构差异，不能继续猜测或只重写配置开关。
- 本轮仅完成发布产物指纹、tag 源码和当前 Codex 工具规划的只读核验，没有修改业务实现。第一类错误在缺少真实请求 payload 时不应贸然改 schema；第二类错误已确认需要扩大运行时数据边界，但应先补能复现工作台 null 注入的测试再实现根修。

## 2026-07-17 v3.16.5-15 打开 MultiRouter 面板立即白屏根修

- 用户补充了决定性触发条件：Provider 主界面正常，只要打开 Codex 多路面板就立即进入错误边界，Safari 报 `null is not an object (evaluating 'e.settingsConfig')`。定向组件测试证明根因不是数据库中的 `settings_config=null`，也不是 React Query 中混入 null provider。
- `CodexRouterWorkspacePage` 在没有有效 MultiRouter 方案时会把 `selectedPlanForModelRefresh` 设为 `null`，随后调用 `readCodexRouting(selectedPlanForModelRefresh)?.routes`。可选链只保护了函数返回值，没有保护传入参数；`readCodexRouting` 内部第一步读取 `provider.settingsConfig`，因此空方案现场必然白屏。
- 根修是让 `readCodexRouting` 的真实契约接受 `Provider | null | undefined` 并在入口返回 `null`。这不是组件旁的临时兜底：该读取函数本来就是“没有路由配置时返回 null”的统一边界，而调用方已经明确允许当前方案为空。
- 新增组件回归：只有普通 Codex provider、没有任何 MultiRouter 方案时渲染工作台，必须正常显示而不能访问 null 的 `settingsConfig`。验证通过工作台 45/45、`pnpm typecheck`、Prettier 和 `git diff --check`。
