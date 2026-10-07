  # CC Switch 代理会话粘性负载均衡设计

  日期：2026-10-07 ｜ 状态：已与用户逐节确认 ｜ 读者：实现本功能的开发者

  ## 1. 定位与价值

  同一应用（Claude Code / Codex 等）配了多个供应商账号时，当前代理的并发能力约等于
  单账号上限：路由是"主备"语义，所有流量打在队首账号，其余账号只在失败时兜底。
  本设计把路由升级为"会话粘性负载均衡"：N 个账号 ≈ N 倍并发额度加和，同时
  同一会话固定走同一账号，保住 prompt cache 命中与 Codex previous_response_id 连续性。

  ## 2. 现状调研（锚点）

  以下事实读自上游 main 分支源码，本地 fork 动工前需逐点校验（fork 有未推送改动）：

  - proxy/provider_router.rs：select_providers 返回按优先级排序的候选列表；
    关闭 failover 时只含当前供应商。语义是"顺序故障转移"，非负载均衡。
  - proxy/failover_switch.rs：FailoverSwitchManager 在 failover 成功后把"当前供应商"
    切过去并推前端/托盘事件，强化单账号承载。
  - database/dao/proxy.rs：AppProxyConfig 仅有 auto_failover_enabled、超时、重试、
    熔断参数，无任何分发/权重/并发字段。
  - proxy/session.rs：extract_session_id 已支持 Claude（x-claude-code-session-id 头 /
    metadata.user_id 的 session 段）与 Codex（session_id 头 / previous_response_id 链），
    SessionId 枚举区分 Extracted / Generated，目前仅用于用量日志。
  - proxy/forwarder.rs：forward_with_retry_inner 按 for provider in providers 顺序重试，
    受 max_attempts 与错误分类约束；活跃连接用 RAII guard（Drop 时异步归还计数）。
  - proxy/handlers.rs + handler_context：请求 body 在 RequestContext::new 之前已解析，
    会话 ID 在 ctx 上，供应商候选列表由 ctx.get_providers() 提供给 forwarder。
  EOF

  第 2 段（决策记录 + 组件设计 + 数据流）：

  ! cat >> docs/superpowers/specs/2026-10-07-session-sticky-load-balance-design.md <<'EOF'

  ## 3. 关键决策记录

  1. 用户诉求确认为"并发能力加和"；行为模式选定：会话粘性优先（方案 B = 粘性轮询 +
     每账号并发上限）。
  2. 账号池 = 现有故障转移队列（复用 UI 与排序语义，不开新配置面）。
  3. 提取不到真实会话 ID（SessionId::Generated）→ 按在途最少选号，不写粘性表。
  4. 粘性命中不受并发上限约束（上限只挡新会话，保缓存连续）；
     粘性账号 429/失败 → 该请求按现有 failover 顺延，成功后会话重映射到实际成功账号。
  5. 并发上限取值：Kimi Max 预设 8（用户指定 2026-10-07；官网定价页 Agent concurrent
     tasks 口径 Max=4，UI tooltip 双标注）；GLM Max 控制台实测 glm-5.3=30、
     glm-5.3-flash=50（用户控制台截图，2026-10-07）；MiMo 官网无数字，留空不限。
     三家官方均为动态调整机制，运行时以 429/403 冷却自适应兜底。

  ## 4. 设计

  ### 4.1 组件

  新增 proxy/load_balance.rs（核心）：
  - 每应用类型一张粘性表 session_id -> provider_id，内存 HashMap，空闲 TTL 30 分钟，
    LRU 上限 4096 条，惰性清理（访问时扫过期，不建后台任务）。
  - 每供应商在途计数 AtomicIsize + InFlightGuard（照抄 ActiveConnectionGuard 的
    Drop 异步归还写法，panic/断连不泄漏）。计数 key = provider_id + outbound model。
  - 并发上限两级：账号默认值 + 按模型覆盖（配置见 4.3）。
  - 选号算法：
    a) 粘性命中且账号仍在池内 → 沿用（不查上限）。
    b) 新会话 → 从轮询游标起找第一个未饱和账号（在途 < 上限，上限 0 = 不限），
       游标推进；全饱和 → 在途最少者放行（本地拒绝会打断客户端，宁可放行让上游 429 兜底）。
    c) 同会话并发抢建映射 → entry().or_insert()，后到者采纳先到者。
    d) 会话 ID 为 Generated → 视为无会话，按在途最少选号，不写表。
  - 429/403 冷却：账号收到 429/403 → 进冷却期（优先 Retry-After，缺省 60s），
    期间新会话选号跳过；连续触发指数退避 60s→2m→5m，封顶 10m。
    与熔断器分工：熔断管错误率/不可用，冷却管限流，互不干扰。

  路由接入（provider_router.rs + handler_context.rs）：
  - 新方法 select_providers_for_session(app_type, session_id)；
    load_balance_mode = off 时行为与现有 select_providers 完全一致。
  - session 模式：池 = 故障转移队列（熔断过滤后），返回 [选中账号] + 其余队列，
    forwarder 零改动，请求内 failover 语义自动保留。
  - RequestContext::new 中把会话提取挪到选号之前（body 此时已解析）。

  迁移回写（forwarder.rs 一处小改）：
  - 实际服务账号 ≠ 路由选中账号（failover 成功）→ 更新该会话粘性映射，
    同时触发 429/403 冷却记录。一套机制覆盖 429/5xx/超时。

  ### 4.2 数据流

  请求 → handlers 解析 body → RequestContext::new：extract_session_id →
  select_providers_for_session（off=现状路径 / session=粘性查表或选号+记表）→
  forwarder 顺序尝试（现状）→ InFlightGuard +1 → 上游 → 响应流结束 guard Drop 计数 -1 →
  failover 发生则会话重映射 + 冷却记录。
  EOF

  第 3 段（配置 + 边界 + 测试 + 分期）：

  ! cat >> docs/superpowers/specs/2026-10-07-session-sticky-load-balance-design.md <<'EOF'

  ### 4.3 配置模型

  - proxy_config 表（key-value，无 schema 迁移）：新增 load_balance_mode
    （"off" | "session"，默认 "off"），复用 update_proxy_config IPC 通道。
  - 实际服务账号 ≠ 路由选中账号（failover 成功）→ 更新该会话粘性映射，
    同时触发 429/403 冷却记录。一套机制覆盖 429/5xx/超时。

  ### 4.2 数据流

  请求 → handlers 解析 body → RequestContext::new：extract_session_id →
  select_providers_for_session（off=现状路径 / session=粘性查表或选号+记表）→
  forwarder 顺序尝试（现状）→ InFlightGuard +1 → 上游 → 响应流结束 guard Drop 计数 -1 →
  failover 发生则会话重映射 + 冷却记录。
  EOF

  第 3 段（配置 + 边界 + 测试 + 分期）：

  ! cat >> docs/superpowers/specs/2026-10-07-session-sticky-load-balance-design.md <<'EOF'

  ### 4.3 配置模型

  - proxy_config 表（key-value，无 schema 迁移）：新增 load_balance_mode
    （"off" | "session"，默认 "off"），复用 update_proxy_config IPC 通道。
  - providers.settings JSON：max_concurrent_requests（u32，0=不限，默认 0）、
    max_concurrent_by_model（map，可选覆盖）。
  - 前端 src/config/codingPlanPresets.ts（新）：每家 × 档位 → 并发值 + 来源标注
    （Kimi：1/1/2/2/8，Max=8 用户指定、官网口径 4 备注于 tooltip；
    GLM：Max 默认 30 + glm-5.3-flash 覆盖 50，控制台实测；MiMo：留空）。
    供应商编辑 UI 并发上限旁加套餐下拉一键填充，可手改。
  - 代理设置页（按应用）加"负载均衡"下拉：关闭 / 会话粘性轮询；i18n 四语言同步
    （en/zh/zh-TW/ja）。
  - 可选打磨：代理页显示"池 N 账号 · 在途 X · 冷却中 Y"实时指示。
    max_concurrent_by_model（map，可选覆盖）。
  - 前端 src/config/codingPlanPresets.ts（新）：每家 × 档位 → 并发值 + 来源标注
    （Kimi：1/1/2/2/8，Max=8 用户指定、官网口径 4 备注于 tooltip；
    GLM：Max 默认 30 + glm-5.3-flash 覆盖 50，控制台实测；MiMo：留空）。
    供应商编辑 UI 并发上限旁加套餐下拉一键填充，可手改。
  - 代理设置页（按应用）加"负载均衡"下拉：关闭 / 会话粘性轮询；i18n 四语言同步
    （en/zh/zh-TW/ja）。
  - 可选打磨：代理页显示"池 N 账号 · 在途 X · 冷却中 Y"实时指示。

  ## 5. 边界与错误处理

  - 粘性账号被熔断 → 池中消失 → 该会话按新会话重选；恢复后新会话会再落回。
  - 重启丢粘性表（内存态）：每活跃会话首个请求冷一次，可接受；不做持久化（YAGNI）。
  - 计数泄漏：guard Drop 异步调度归还，复用已验证模式。
  - Codex previous_response_id：粘性让断链只在账号真失败时发生，优于现状。
  - Kimi 403 并发限制：按 429 同路径进冷却（错误分类处加 403 concurrent limit 识别）。

  ## 6. 测试与验收

  - Rust 单测：选号全分支（粘性命中/过期/驱逐/饱和跳过/全饱和兜底/并发抢建/
    Generated 不入表）、两级上限查表、session 模式排序、failover 重映射、冷却退避。
  - 集成：双 mock 上游验证 ①N 并发会话分到两账号 ②同会话恒定同账号 ③杀一号后
    会话迁移 ④429 后新会话避开冷却账号。
  - 门禁：cargo test + cargo clippy -D warnings + pnpm typecheck + pnpm test:unit 全绿。
  - 手工验收：双账号跑 Claude Code 多会话，代理页两账号用量同涨、单会话缓存不回退。

  ## 7. 分期

  - 阶段 1（方案 A 范围）：load_balance_mode 字段 + 粘性表 + 路由接入 + 重映射。
  - 阶段 2（B 增量）：InFlightGuard + 两级并发上限 + 429/403 冷却 + 预设表与 UI。
  - 每阶段测试绿后各提交一次 commit（Conventional Commits）。
  EOF

  三段跑完后验证再提交：

  ! wc -l docs/superpowers/specs/2026-10-07-session-sticky-load-balance-design.md && head -3 docs/superpowers/specs/2026-10-07-session-sticky-load-balance-design.md

  ! git add docs/superpowers/specs/2026-10-07-session-sticky-load-balance-design.md && git commit -m "docs(proxy): 会话粘性负载均衡设计文档" -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"

✻ Cooked for 38s · done 3:19 PM

※ recap: 正在为 CC Switch 做会话粘性负载均衡设计，多账号并发加和，设计已全部定稿。下一步：按顺序跑我给出的三段落盘命令，把设计文档写入 docs/superpowers/specs/，再跑验证和 commit。

!  wc -l docs/superpowers/specs/2026-10-07-session-sticky-load-balance-design.md && head -3 docs/superpowers/specs/2026-10-07-session-sticky-load-balance-design.md
  ⎿  wc: docs/superpowers/specs/2026-10-07-session-sticky-load-balance-design.md: open: No such file or directory


  Thought for 37s (ctrl+o to expand)

⏺ 文件还是不存在——三条写入命令都没执行成功。我怀疑是 Claude Code 输入框对多行 heredoc 粘贴支持不好（粘贴时只剩首行或被吞）。

  换个稳妥路子：在你自己的终端里跑（就是你装 Chrome 用的那个终端，多行粘贴没问题）。步骤：

  1. 打开终端，执行 cd /Volumes/nvme2230/lab/cc-switch
  2. 把下面整块粘贴进去回车（写入 + 提交一次完成）：

  mkdir -p docs/superpowers/specs && cat > docs/superpowers/specs/2026-10-07-session-sticky-load-balance-design.md <<'EOF'
  # CC Switch 代理会话粘性负载均衡设计

  日期：2026-10-07 ｜ 状态：已与用户逐节确认 ｜ 读者：实现本功能的开发者

  ## 1. 定位与价值

  同一应用（Claude Code / Codex 等）配了多个供应商账号时，当前代理的并发能力约等于
  单账号上限：路由是"主备"语义，所有流量打在队首账号，其余账号只在失败时兜底。
  本设计把路由升级为"会话粘性负载均衡"：N 个账号 ≈ N 倍并发额度加和，同时
  同一会话固定走同一账号，保住 prompt cache 命中与 Codex previous_response_id 连续性。

  ## 2. 现状调研（锚点）

  以下事实读自上游 main 分支源码，本地 fork 动工前需逐点校验（fork 有未推送改动）：

  - proxy/provider_router.rs：select_providers 返回按优先级排序的候选列表；
    关闭 failover 时只含当前供应商。语义是"顺序故障转移"，非负载均衡。
  - proxy/failover_switch.rs：FailoverSwitchManager 在 failover 成功后把"当前供应商"
    切过去并推前端/托盘事件，强化单账号承载。
  - database/dao/proxy.rs：AppProxyConfig 仅有 auto_failover_enabled、超时、重试、
    熔断参数，无任何分发/权重/并发字段。
  - proxy/session.rs：extract_session_id 已支持 Claude（x-claude-code-session-id 头 /
    metadata.user_id 的 session 段）与 Codex（session_id 头 / previous_response_id 链），
    SessionId 枚举区分 Extracted / Generated，目前仅用于用量日志。
  - proxy/forwarder.rs：forward_with_retry_inner 按 for provider in providers 顺序重试，
    受 max_attempts 与错误分类约束；活跃连接用 RAII guard（Drop 时异步归还计数）。
  - proxy/handlers.rs + handler_context：请求 body 在 RequestContext::new 之前已解析，
    会话 ID 在 ctx 上，供应商候选列表由 ctx.get_providers() 提供给 forwarder。

  ## 3. 关键决策记录

  1. 用户诉求确认为"并发能力加和"；行为模式选定：会话粘性优先（方案 B = 粘性轮询 +
     每账号并发上限）。
  2. 账号池 = 现有故障转移队列（复用 UI 与排序语义，不开新配置面）。
  3. 提取不到真实会话 ID（SessionId::Generated）→ 按在途最少选号，不写粘性表。
  4. 粘性命中不受并发上限约束（上限只挡新会话，保缓存连续）；
     粘性账号 429/失败 → 该请求按现有 failover 顺延，成功后会话重映射到实际成功账号。
  5. 并发上限取值：Kimi Max 预设 8（用户指定 2026-10-07；官网定价页 Agent concurrent
     tasks 口径 Max=4，UI tooltip 双标注）；GLM Max 控制台实测 glm-5.3=30、
     glm-5.3-flash=50（用户控制台截图，2026-10-07）；MiMo 官网无数字，留空不限。
     三家官方均为动态调整机制，运行时以 429/403 冷却自适应兜底。

  ## 4. 设计

  ### 4.1 组件

  新增 proxy/load_balance.rs（核心）：
  - 每应用类型一张粘性表 session_id -> provider_id，内存 HashMap，空闲 TTL 30 分钟，
    LRU 上限 4096 条，惰性清理（访问时扫过期，不建后台任务）。
  - 每供应商在途计数 AtomicIsize + InFlightGuard（照抄 ActiveConnectionGuard 的
    Drop 异步归还写法，panic/断连不泄漏）。计数 key = provider_id + outbound model。
  - 并发上限两级：账号默认值 + 按模型覆盖（配置见 4.3）。
  - 选号算法：
    a) 粘性命中且账号仍在池内 → 沿用（不查上限）。
    b) 新会话 → 从轮询游标起找第一个未饱和账号（在途 < 上限，上限 0 = 不限），
       游标推进；全饱和 → 在途最少者放行（本地拒绝会打断客户端，宁可放行让上游 429 兜底）。
    c) 同会话并发抢建映射 → entry().or_insert()，后到者采纳先到者。
    d) 会话 ID 为 Generated → 视为无会话，按在途最少选号，不写表。
  - 429/403 冷却：账号收到 429/403 → 进冷却期（优先 Retry-After，缺省 60s），
    期间新会话选号跳过；连续触发指数退避 60s→2m→5m，封顶 10m。
    与熔断器分工：熔断管错误率/不可用，冷却管限流，互不干扰。

  路由接入（provider_router.rs + handler_context.rs）：
  - 新方法 select_providers_for_session(app_type, session_id)；
    load_balance_mode = off 时行为与现有 select_providers 完全一致。
  - session 模式：池 = 故障转移队列（熔断过滤后），返回 [选中账号] + 其余队列，
    forwarder 零改动，请求内 failover 语义自动保留。
  - RequestContext::new 中把会话提取挪到选号之前（body 此时已解析）。

  迁移回写（forwarder.rs 一处小改）：
  - 实际服务账号 ≠ 路由选中账号（failover 成功）→ 更新该会话粘性映射，
    同时触发 429/403 冷却记录。一套机制覆盖 429/5xx/超时。

  ### 4.2 数据流

  请求 → handlers 解析 body → RequestContext::new：extract_session_id →
  select_providers_for_session（off=现状路径 / session=粘性查表或选号+记表）→
  forwarder 顺序尝试（现状）→ InFlightGuard +1 → 上游 → 响应流结束 guard Drop 计数 -1 →
  failover 发生则会话重映射 + 冷却记录。

  ### 4.3 配置模型

  - proxy_config 表（key-value，无 schema 迁移）：新增 load_balance_mode
    （"off" | "session"，默认 "off"），复用 update_proxy_config IPC 通道。
  - providers.settings JSON：max_concurrent_requests（u32，0=不限，默认 0）、
    max_concurrent_by_model（map，可选覆盖）。
  - 前端 src/config/codingPlanPresets.ts（新）：每家 × 档位 → 并发值 + 来源标注
    （Kimi：1/1/2/2/8，Max=8 用户指定、官网口径 4 备注于 tooltip；
    GLM：Max 默认 30 + glm-5.3-flash 覆盖 50，控制台实测；MiMo：留空）。
    供应商编辑 UI 并发上限旁加套餐下拉一键填充，可手改。
  - 代理设置页（按应用）加"负载均衡"下拉：关闭 / 会话粘性轮询；i18n 四语言同步
    （en/zh/zh-TW/ja）。
  - 可选打磨：代理页显示"池 N 账号 · 在途 X · 冷却中 Y"实时指示。

  ## 5. 边界与错误处理

  - 粘性账号被熔断 → 池中消失 → 该会话按新会话重选；恢复后新会话会再落回。
  - 重启丢粘性表（内存态）：每活跃会话首个请求冷一次，可接受；不做持久化（YAGNI）。
  - 计数泄漏：guard Drop 异步调度归还，复用已验证模式。
  - Codex previous_response_id：粘性让断链只在账号真失败时发生，优于现状。
  - Kimi 403 并发限制：按 429 同路径进冷却（错误分类处加 403 concurrent limit 识别）。

  ## 6. 测试与验收

  - Rust 单测：选号全分支（粘性命中/过期/驱逐/饱和跳过/全饱和兜底/并发抢建/
    Generated 不入表）、两级上限查表、session 模式排序、failover 重映射、冷却退避。
  - 集成：双 mock 上游验证 ①N 并发会话分到两账号 ②同会话恒定同账号 ③杀一号后
    会话迁移 ④429 后新会话避开冷却账号。
  - 门禁：cargo test + cargo clippy -D warnings + pnpm typecheck + pnpm test:unit 全绿。
  - 手工验收：双账号跑 Claude Code 多会话，代理页两账号用量同涨、单会话缓存不回退。

  ## 7. 分期

  - 阶段 1（方案 A 范围）：load_balance_mode 字段 + 粘性表 + 路由接入 + 重映射。
  - 阶段 2（B 增量）：InFlightGuard + 两级并发上限 + 429/403 冷却 + 预设表与 UI。
  - 每阶段测试绿后各提交一次 commit（Conventional Commits）。