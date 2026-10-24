# CC Switch 用量多维度 详细设计评审记录

## 最新复核：Oh My Pi 与数据库版本冲突（2026-10-24）

合并上游 `main` `cae7c375`，解决 `database/schema.rs` 和 `database/tests.rs` 的冲突。上游新增 Oh My Pi，使用 v22→v23 增加 MCP 的 `enabled_omp`；本功能此前也使用 v23。仅消除文本冲突会使上游 v23 用户跳过多维迁移，也会使早期 PR v23 用户缺少 MCP 开关。

- 保留上游 v22→v23 迁移，将多维归属迁移独立调整为 v23→v24；迁移入口同时补齐早期 PR v23 缺少的 `enabled_omp`。
- 保留上游 MCP 迁移测试及本功能的迁移、数据完整性测试。新增上游 v23 升级、早期 PR v23 开关修复和幂等性回归；归属与汇总保留测试覆盖早期 PR v21、v23，确认项目、数据源分组和已有归属证据不被覆盖。
- 两项新增回归在修复前实际失败；修复后数据库 schema 测试 18 项通过。独立复核未发现高、中优先级问题。

验证结果：

- `pnpm test:unit --maxWorkers=2 --minWorkers=1`：260 个文件、3302 项通过。
- `pnpm typecheck`、`pnpm format:check`、`pnpm build:renderer`：通过。
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`：通过。
- 完整 `cargo test --manifest-path src-tauri/Cargo.toml`：4255 项通过，含 4047 项库测试和 208 项集成测试；12 项忽略，0 失败。使用隔离测试目录、`umask 022` 和 `NO_PROXY=127.0.0.1,localhost`；仅在测试子进程的 PATH 中移除 Bun 全局 bin，复现 GUI 启动环境。

首轮完整 Rust 测试有 1 项失败：上游新增 `omp_search_paths_include_the_bun_global_bin` 假定 GUI 的 PATH 不含 Bun 全局 bin，当前终端预含该目录，导致 Gemini 的负断言失败。应用搜索实现与上游一致；在上述子进程环境中，该项及完整测试集均通过，未修改应用搜索逻辑或系统环境。后续从终端运行此类搜索路径用例时，应显式区分继承的终端 PATH 与 GUI PATH。

## 上一轮复核：下钻范围与历史归属（2026-10-24）

复查确认三类问题，均先由回归测试复现，再修复：

1. 供应商下钻未继承已选模型，项目下钻未继承已选供应商和模型，展开后会显示父行范围以外的数据。两个入口传递完整筛选；项目供应商统计复用支持组合过滤的接口，局部选择随父级范围变化重置。4 项交互回归在修复前失败，修复后通过，覆盖 Token 与缓存视图。
2. 请求详情忽略已保存的供应商名称快照，卡片改名后显示新名称。详情改为快照优先，汇总及筛选菜单仍使用当前卡片名。新增改名前后名称不同的回归，修复前失败、修复后通过。
3. Codex 启动回填只检查占位供应商行，因去重仅剩真实代理行时会提前标记完成；运行时项目证据晚到也不会补齐既有失败请求。v2 一次性回填覆盖这类旧代理行并规范化会话键，项目索引写入同时补齐同应用、同会话的空项目。两项回归先复现失败，再验证旧标记升级、重复回填短路、失败请求补齐，以及供应商和既有项目不被覆盖。

验证结果：

- `pnpm test:unit --maxWorkers=2 --minWorkers=1`：258 个文件、3276 项通过。
- `pnpm typecheck`、`pnpm format:check`、`pnpm build:renderer`：通过。
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`：通过。
- 隔离测试目录并设置 `NO_PROXY=127.0.0.1,localhost` 后运行完整 `cargo test`：4206 项通过，含 4003 项库测试和 203 项集成测试；12 项忽略，0 失败。
- 与最新上游 `main` `d8983bb1` 的文本合并预检通过。

## 上一轮复核：用量页面改版合并（2026-10-23）

合并上游 `main` `717172bd`，解决 Kimi 同步器、用量面板、供应商表、模型表、请求详情、请求日志与图表颜色配置共 7 个文件的文本冲突。保留上游新布局、级联筛选、设置抽屉、日志分页收缩和列分组，同时保留项目维度、供应商身份隔离、缓存下钻与归属信息。

- 级联模型查询继续使用原始供应商 ID、应用类型和项目；新增同名、同 ID 跨应用的回归，确认只显示所选供应商对应的模型，概览和日志沿用相同过滤。
- 项目页签接入上游计数布局时，`useProjectStats` 原先忽略 `enabled`，导致未打开页签就查询；新增回归先复现 1 次意外调用，修复后未打开时不查询，打开后正常加载。
- 请求详情新增原生会话回归，验证项目、归属来源、置信度和原因仍可见；保留上游隐藏会话内部 ID 的行为。
- Kimi 保留上游解析器调整及账号能力，同时保留原生会话的未识别标记与重复同步修复；5 项 Kimi 测试通过。

验证结果：

- `pnpm test:unit --maxWorkers=2 --minWorkers=1`：257 个文件、3271 项通过。
- `pnpm typecheck`、`pnpm format:check`、`pnpm build:renderer`：通过。
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`：通过。
- 使用隔离测试目录，运行 `NO_PROXY=127.0.0.1,localhost cargo test --manifest-path src-tauri/Cargo.toml`：4204 项通过，含 4001 项库测试和 203 项集成测试；12 项忽略，0 失败。

## 上一轮复核：Kimi 与迁移兼容（2026-10-22）

合并上游 `main` `e4873bc8`。文本自动合并成功，但组合后的代码存在三个实际问题，均已增加回归并修复：

1. Kimi 新增的两处统计测试仍使用旧参数数量，`cargo test --lib session_usage_kimi` 编译报 `E0061`。补齐项目、供应商应用和归属过滤参数后，6 项 Kimi 测试通过。
2. Kimi 原生会话 INSERT 未指定归属列，会继承 `proxy/exact` 默认值；新回归在修复前实际读到该错误值。新写入和已有记录刷新均明确标记 `unknown/unknown/kimi_native_session`，重复同步保持明细数量不变。
3. 上游已占用 schema v21/v22，原先放在 v20→v21 的多维迁移会被这些用户跳过；升级回归在修复前因缺少 `provider_activations` 失败。现在保留上游迁移，并使用独立 v22→v23；已有多维归属不降级，汇总保留项目和数据源分组，只有缺少维度时才重建。5 项专用回归覆盖上游 v20/v21/v22、部分汇总维度及早期 PR v21 数据。

当前分支验证：

- `pnpm test:unit --maxWorkers=2 --minWorkers=1`：253 个文件、3210 项通过。
- `pnpm typecheck`、`pnpm format:check`、`pnpm build:renderer`：通过。
- `cargo fmt --check`、`cargo clippy -- -D warnings`：通过。
- `cargo test --lib database::schema`：16 项通过；`cargo test --lib session_usage_kimi`：6 项通过；`cargo test --lib usage_stats`：47 项通过。
- 隔离测试目录后运行 `NO_PROXY=127.0.0.1,localhost cargo test --manifest-path src-tauri/Cargo.toml`：4169 项通过、12 项忽略、0 失败，含 3970 项库测试和 199 项集成测试。

完整 Rust 首轮：3969 通过、1 失败、12 忽略；失败是上游原有的 `describe_reqwest_error_keeps_cause_and_drops_url`，预期本机空闲端口拒绝连接，实际收到 502。该文件与当前 main 无差异。仅对测试进程设置 `NO_PROXY=127.0.0.1,localhost` 后，单项复测通过；本机此类网络用例需明确回环地址不走代理，再运行测试。未修改应用网络设置或该用例。

## 上一轮复核：模型缓存下钻（2026-10-22）

模型缓存下钻及原有多维用量变更已与上游 `main` 的 `cb90a244` 合并。没有文本冲突；启动逻辑、语言文件及 4.0.18 版本信息均保留上游更新，项目和供应商下钻继续使用原始应用与供应商卡片 ID 过滤。

- `pnpm test:unit --maxWorkers=2 --minWorkers=1`：251 个文件、3180 项通过。
- `pnpm typecheck`、`pnpm format:check`、`pnpm build:renderer`：通过。
- `cargo fmt --check`、`cargo clippy -- -D warnings`：通过。
- `cargo test --lib usage_stats`：47 项通过；供应商回填回归用例：1 项通过。
- Claude 会话解析的 `parse_session` 回归：9 项通过，覆盖上游新增的最后一次 `/cd` 归组行为。
- 缓存视图使用本地样例检查桌面、390 像素视口及深浅主题；完整 Rust 测试集本轮未重跑。

异常复验：首次默认并发的全量前端运行中，图标选择器等待异步图标按钮超时；该文件独立复测 10 项通过，限制并发后的完整测试集全部通过。未修改图标业务实现，尚不足以把该现象归因为产品缺陷。后续遇到同类超时，先在相同并发和异步加载条件下复核，再决定是否修改业务代码。

## 1. 评审对象

| 项目 | 内容 |
| --- | --- |
| 设计文档 | `docs/design/usage-multidim/usage-multidim-design-zh.md` |
| 上游 | `docs/plan/usage-multidim/usage-multidim-requirements-zh.md`、`docs/review/usage-multidim/usage-multidim-review-zh.md` |
| 评审方式 | 设计条目对照当前源码逐条核实（模式系统、切换入口、DAO/命令/前端文件、时间单位、rollup 路径） |
| 结论 | 发现 3 项高、3 项中、2 项低问题，已全部修入设计文档；修正后可进入开发 |
| 最后更新 | 2026-10-20 |

## 2. 发现与处理

### D-01 设计假设存在“聚合模式”，代码里不存在

严重程度：高

问题：设计 4.1 写“直连、路由、聚合模式发生切换时”。实际模式枚举只有 `Mode::Direct | Mode::Proxy`（`src-tauri/src/mode/state.rs:54`）；故障转移和多供应商栈路由都属于 `Mode::Proxy`。

处理：设计 4.1 已改为模式取值 `direct | proxy`，并说明栈路由仍属 proxy。

结论：已解决。

### D-02 激活时间线写入钩子位置错误且不完整

严重程度：高

问题：设计写“写入点在 `switch_provider` 成功后的 `set_current_provider`”。两个问题：其一，`set_current_provider` 是 DAO，会被官方 seed、导入兜底、测试路径误触发；其二，`ProviderService::switch_inner` 同一入口既处理直连切换、也处理代理模式下只改 `proxy_route` 的路由切换，钩子必须区分当前模式；其三，进入/退出代理走 `mode/operation.rs` 是独立路径，设计没有覆盖。

处理：设计 4.1 改为 service 层四个入口表（`ProviderService::switch` 的 Ok 分支、`import_default_config`、`mode/operation.rs::run`/`commit_target`、`recover_on_startup`），各自标注 source 与 mode/provider_id 语义。

结论：已解决。

### D-03 时间单位不一致未说明

严重程度：高

问题：`proxy_request_logs.created_at` 为秒级，`provider_activations.effective_at_ms` 为毫秒级。归属查询直接比较会差三个数量级，设计未写换算。

处理：设计 4.1 末尾明确统一换算为毫秒（`start_s * 1000`），同秒歧义判定按秒对齐。

结论：已解决。

### D-04 provider_name_snapshot 写入时机缺失

严重程度：中

问题：需求 9.3 要求新写入行在请求发生时保存供应商名称快照，设计只写了迁移回填，没有写 insert 路径（各 `session_usage*.rs` 的 INSERT 和代理写日志处）。

处理：设计新增 4.5，明确两条写入路径和读侧优先级。

结论：已解决。

### D-05 日汇总 rollup 改造只列了步骤没有 SQL 口径

严重程度：中

问题：`rollup_and_prune` 的 GROUP BY 要加 `project_key`，快照列要从明细行带出；设计第 7 节只有一句“日汇总项目维度与快照”。

处理：设计新增 5.4，明确 GROUP BY 口径与快照带出规则。

结论：已解决。

### D-06 存量会话行无法通过重跑同步重新归属

严重程度：中

问题：会话同步有 `session_usage_dedup` 去重账本，已导入请求不会重导。功能上线后历史会话行永远保持未识别，设计没有写清这个边界，开发容易误以为重跑同步能回填。

处理：设计新增 4.6，明确该边界与需求 R-10 一致。

结论：已解决（边界声明）。

### D-07 设计原则与本次目标冲突

严重程度：低

问题：设计原则 6 写“本次只落地数据模型与归属基础设施”，与“完成 P0 全量”的实际目标冲突。

处理：原则 6 已改为“本次落地 P0 全量”。

结论：已解决。

### D-08 设计文档行号引用漂移

严重程度：低

问题：设计引用的 `schema.rs:280`、`schema.rs:1370` 已因 v21 迁移代码合入而漂移。

处理：已校准为 `schema.rs:288`、`schema.rs:1637`。

结论：已解决。

## 3. 核对通过项

- 新表/新列 DDL 与已合入的 `migrate_v20_to_v21` 一致；迁移对测试最小表做了防御，真实库列齐全时路径不变。
- 归属算法与需求 FR-01/FR-02、评审 R-22 的 write-side skip 描述一致，去重两条路径的调用点行号经核实准确。
- 前端文件 `UsageDashboard.tsx`、`ProviderStatsTable.tsx`、`RequestDetailPanel.tsx`、`src/lib/api/usage.ts`、`LogFilters`（`src/types/usage.ts:157`）均存在，设计的前端改动面可落地。
- 代理 session_id 提取路径（`src-tauri/src/proxy/session.rs`）覆盖 Claude/Codex/Grok 的 header 与 metadata 来源，session_key 规范化有落点。
- 客户端能力声明所需的 `AppType::is_additive_mode()`（`src-tauri/src/app_config.rs`）已存在，切换式/累加式判断有据。

## 4. 评审结论

设计文档按上述 8 项修正后，与当前代码事实一致，可以进入开发。开发门禁沿用需求评审的 8 项，另加本次两项：

1. 激活时间线写入必须覆盖 4.1 的四个入口，且代理模式下的切换记录 `mode='proxy'`。
2. 归属查询的时间比较必须统一为毫秒，单元测试覆盖同秒歧义。

## 5. 实现后复核（2026-10-20）

实现已完成，复核结果：

- 激活时间线实际落在 `ProviderService::switch`、`mode::operation::commit_target` 与启动基线三处；代理模式切换记录 `mode='proxy'`（与 D-02 一致）。
- 时间统一按毫秒比较，`resolve_provider_attribution` 覆盖无激活、代理模式、区间跨切换、同秒边界四类用例。
- 去重改为 `skip_if_duplicate_with_project` / `backfill_if_proxy_duplicate`，命中代理行时先回写项目再跳过（与 R-22 一致）。
- 代理写入补充 `session_key`、项目关联与供应商名称快照；会话适配器均显式写归属列，避免默认值误标。
- 真实数据副本迁移（v20→v21）不丢行；项目、供应商、缓存与日志查询在副本上跑通。
- 自动化验证：Rust 3833 通过（1 个与本改动无关的本机网络用例失败）、前端 3068 用例通过、`typecheck` 与 renderer build 通过。

结论：实现与最终需求、设计一致，P0 范围完成。
