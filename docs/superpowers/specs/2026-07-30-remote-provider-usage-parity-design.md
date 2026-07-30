# 远程 Provider 与使用统计等价能力设计

## 1. 背景

CC Switch 已具备本机/SSH 远程运行目标切换、临时 Agent 投送和第一阶段 Provider RPC，
但当前实现尚未达到“功能相同，仅操作主机不同”的语义：

- `cc-switch-core` 自建的最小 Provider schema 使用 `providers.app` 和
  `current_providers`，桌面真实数据库使用 `providers.app_type` 与 `is_current`。
- Agent 即使打开远端已有的 `~/.cc-switch/cc-switch.db`，也无法可靠读取桌面版保存的
  Provider、端点和当前状态。
- Headless Provider DTO 缺少图标、端点、故障转移状态等桌面字段，支持的应用类型和 live
  配置写入也不完整。
- 使用统计前端 API 仍直接调用本机 Tauri `invoke`，Agent 能力表没有 Usage 命令；切换目标后
  虽然清除了查询缓存，重新请求仍然读取本机数据库。
- 运行目标切换期间的迟到响应没有目标命名空间，存在旧主机数据覆盖当前页面的风险。

## 2. 目标

本阶段建立 Provider 与整个使用统计页面的远程等价能力：

- 选择本机时继续操作本机数据库和 live 配置。
- 选择远程主机时，只操作该主机的数据库、CLI 配置和会话日志。
- 远端已有 CC Switch 数据库时，完整显示已有 Provider、当前 Provider 和使用统计。
- Provider 列表、当前项、增删改、排序、切换与 live 配置写入在两种目标下保持相同语义。
- 使用统计汇总、趋势、Provider/模型统计、请求日志、详情、数据来源、模型定价、限额、
  会话同步和 Codex 重建在两种目标下保持相同语义。
- 本机与各远程目标的数据、查询缓存和迟到响应严格隔离。
- Agent 继续保持无 Tauri、GTK、WebKit 依赖和临时投送模式。

## 3. 非目标

本阶段不同时扩展 MCP、Prompts、Skills、Profiles、代理、会话浏览、同步、备份、认证和订阅
页面。这些领域继续采用显式能力白名单，未迁移命令在远程模式返回 `COMMAND_NOT_EXPOSED`
或 `CAPABILITY_UNAVAILABLE`，不得静默落回本机执行。

本阶段不让 Agent 调用远端已安装的 CC Switch 可执行文件。远端安装状态和版本不可作为
协议前提，桌面端内嵌的临时 Agent 仍是唯一远程执行入口。

## 4. 架构选择

采用共享规范化 Headless Core：Provider 与 Usage 的 DTO、数据库访问和业务规则下沉到
`cc-switch-core`，桌面 Tauri 命令与 `cc-switch-agent` 调用同一服务。Agent 只负责 HOME
解析、协议、能力白名单和错误传输，不复制 SQL 或业务规则。

不采用以下方案：

- Agent 单独复制桌面 SQL：短期改动小，但本地和远程会随数据库演进再次分叉。
- 调用远端 CC Switch/CLI：无法保证安装和版本一致，且破坏零安装的临时 Agent 边界。
- 让 Agent 链接桌面 crate：会引入 Tauri 和 Linux GUI 依赖，无法维持静态最小二进制。

## 5. 数据库兼容与状态边界

### 5.1 规范 schema

删除 Headless Core 当前的 `providers.app/current_providers` 私有 schema。Core 使用桌面数据库
v16 的规范结构，包括：

- `providers.app_type`、`providers.is_current` 和完整 Provider 列。
- `provider_endpoints`。
- `usage_logs`、`usage_rollups`、`model_pricing` 及 Usage 查询依赖的规范表和列。
- `PRAGMA user_version` 作为兼容性信号，不再用 Agent 自定义 schema 版本冒充桌面版本。

Provider 与 Usage 数据访问实现为可接受 `rusqlite::Connection` 的共享服务。桌面端保留现有
连接生命周期和锁，Agent 的 `HeadlessState` 持有远端数据库连接，两者复用同一 SQL 和 DTO。

### 5.2 打开已有数据库

Agent 打开已有 `~/.cc-switch/cc-switch.db` 时先读取 `user_version` 并验证本阶段所需表、列和
索引。连接阶段只做只读兼容检查，不静默迁移、重命名或覆盖已有表。

数据库版本过新、版本过旧或结构缺失时返回 `DATABASE_INCOMPATIBLE`，错误包含检测版本和
Agent 支持版本，但不输出数据库内容。连接保持失败或对应能力禁用，不允许部分 Provider
写入落到未知结构。

### 5.3 没有数据库的远端

远端不存在数据库时，Core 创建与桌面当前版本一致的规范表，不创建第二套最小格式。新库
完成初始化后设置相同 `user_version`，后续远端安装桌面 CC Switch 时可直接继续使用。

SQLite 连接启用外键、合理 busy timeout 和事务边界，以兼容远端同时运行的 CC Switch
进程。锁冲突返回结构化忙碌错误，不通过无限重试阻塞 SSH 会话。

## 6. Provider 等价能力

### 6.1 数据模型

共享 Provider DTO 覆盖桌面字段：ID、名称、设置、网址、分类、创建时间、排序、备注、图标、
图标颜色、meta、自定义端点和故障转移状态。支持以下应用标识：

- `claude`
- `claude-desktop`
- `codex`
- `gemini`
- `grokbuild`
- `opencode`
- `openclaw`
- `hermes`

### 6.2 操作语义

开放 `provider.list`、`provider.current`、`provider.add`、`provider.update`、
`provider.delete`、`provider.update_sort_order` 和 `provider.switch`。

新增、编辑、删除和排序沿用桌面验证规则。切换先在 SQLite 事务中更新 `is_current`，事务
提交后再写目标主机的 live 配置。live 写入失败必须返回 `LIVE_WRITE_FAILED`；UI 不显示假
成功，并通过后续刷新展示数据库的真实当前状态。

### 6.3 live 配置

各应用的 live writer 下沉为不依赖 Tauri 的服务，并显式接收远端 HOME/路径上下文。Claude、
Codex、Gemini、Grok Build、OpenCode、OpenClaw 和 Hermes 使用各自现有的文件格式与
累加/独占语义，不用通用 JSON 覆盖实现代替。

`claude-desktop` Provider 数据仍可在远端列出和管理，但 Linux 目标没有 Claude Desktop
集成时，切换和 live 写入必须在数据库事务前返回 `CAPABILITY_UNAVAILABLE`，不得修改
`is_current` 或伪造本机路径。未来仅在远端平台实际具备对应集成时开放该条件能力。

live 文件采用同目录临时文件、写入、flush 和原子替换流程；保留既有安全校验、公共配置
片段和应用特定规则。任何路径均从 Agent 的远端 HOME 推导，禁止读取本机路径或再次依赖
进程全局 HOME。

## 7. 使用统计等价能力

### 7.1 查询能力

共享 Usage DTO、SQL helper 和聚合规则，开放：

- `usage.summary`
- `usage.summary_by_app`
- `usage.trends`
- `usage.provider_stats`
- `usage.model_stats`
- `usage.logs`
- `usage.detail`
- `usage.data_sources`
- `usage.pricing.list`
- `usage.limits`
- `usage.provider_query`

所有日期、过滤、分页、token 语义、Provider 名称回退、费用和数据来源规则与桌面命令共用
实现。日志继续分页传输，单帧必须受协议 16 MiB 上限保护。

### 7.2 写入和长任务

开放：

- `usage.pricing.update`
- `usage.pricing.delete`
- `usage.provider_test`
- `usage.session_sync`
- `usage.codex_rebuild`

模型定价更新沿用非负校验和历史费用回填。会话同步读取目标主机自己的 CLI 会话目录。
Codex 重建在目标主机先备份数据库，再执行 reset 与重新导入；失败不得删除备份或报告成功。

普通查询默认超时 30 秒。会话同步和 Codex 重建默认超时 5 分钟，携带 operation ID，属于
不可自动重试写操作；断线或超时返回明确状态，连接恢复后由用户决定是否重新执行。

## 8. 协议与能力注册

`CommandCapabilityRegistry` 从 `provider_phase()` 扩展为按领域组合的注册表。每项声明命令
名称、只读性、幂等性和默认超时。握手返回当前 Agent 实际能力集合，桌面后端和 Agent 使用
同一注册表校验命令，防止两端白名单漂移。

Agent 将 `dispatch_provider_command` 升级为统一 `dispatch_command`，再委托 Provider 或 Usage
服务。未登记命令继续返回 `COMMAND_NOT_EXPOSED`，不得根据字符串反射调用桌面命令。

每个远程请求携带运行环境 generation。桌面后端在发送前和接收后都核对当前 generation；
目标已经切换时返回 `STALE_RUNTIME`，丢弃迟到结果。写请求不会跨 generation 重放。

## 9. 前端路由与缓存隔离

`usageApi` 全部改用 `appInvoke`：本机目标调用原 Tauri 命令，在线远程目标调用对应
`usage.*` 命令，远程离线或能力缺失时显示结构化错误。任何 Usage 方法都不能静默回退到
本机。

Provider 与 Usage 查询键加入运行目标 ID 和 generation。切换目标时：

1. 发布 connecting generation 并取消旧查询。
2. 关闭旧 SSH 会话并连接新目标。
3. 发布 online/local 快照，清空目标相关缓存。
4. 使用新目标命名空间重新请求数据。

自身 Provider/Usage 写操作成功后立即失效对应目标缓存。远端其他 CC Switch 进程写入的
Usage 记录由现有 30 秒轮询读取；本阶段不为外部进程数据库写入增加文件监听事件。

## 10. 错误与安全语义

新增或稳定以下错误码：

- `DATABASE_INCOMPATIBLE`
- `DATABASE_BUSY`
- `COMMAND_NOT_EXPOSED`
- `CAPABILITY_UNAVAILABLE`
- `REMOTE_OFFLINE`
- `STALE_RUNTIME`
- `LIVE_WRITE_FAILED`
- `REMOTE_PERMISSION_DENIED`
- `REMOTE_OPERATION_TIMEOUT`

错误消息继续移除控制字符并限制长度。Provider settings、API key、token、请求详情中的敏感
配置和完整远端路径不得写入普通日志。Agent 只接受当前 SSH stdio 会话，不增加监听端口或
持久服务。

## 11. 测试策略

### 11.1 数据库兼容

- 使用桌面 v16 真实 schema fixture，而不是 Agent 私有内存 schema。
- fixture 包含 Provider 完整字段、端点、当前状态、Usage 记录和模型定价。
- 验证 Agent 打开已有库后 schema、user_version 和数据均未改变。
- 验证缺失库创建规范 schema，过新/过旧/缺列数据库返回兼容错误。

### 11.2 Provider

- 覆盖八种应用的列表、当前项、增删改和排序；七种 Linux CLI 应用覆盖切换与 live 写入。
- 验证 Linux 上切换 `claude-desktop` 在数据库变更前返回 `CAPABILITY_UNAVAILABLE`。
- 检查切换只修改目标数据库和目标 live 文件。
- 检查 live 写入失败、当前项删除、数据库锁和权限错误。
- 保留 Agent 依赖审计，禁止引入 Tauri/GTK/WebKit。

### 11.3 Usage

- 同一 fixture 分别通过桌面服务和 Agent RPC 查询，逐字段比较结果。
- 覆盖日期边界、过滤、分页、token 语义、费用、数据来源和 Provider 名称回退。
- 覆盖定价校验与回填、Provider 限额、会话同步、Codex 备份/重建、超时与失败恢复。

### 11.4 协议与前端

- 测试能力白名单、超时元数据、稳定错误码和 generation 拒绝。
- 前端 mock 测试保证所有 Usage API 依据目标路由，查询键包含目标命名空间。
- 测试本机、远程在线、连接中、离线、不兼容和切换期间的禁用状态。

### 11.5 真实服务器验收

在 `root@172.16.0.108` 上使用临时 Agent 验证：

- 显示远端已有 Provider 和当前项。
- 切换 Provider 只改变远端数据库与 live 配置。
- 使用统计读取远端数据，切回本机后恢复本机数据。
- 断线后 `/tmp` 不残留 Agent。

验收过程不输出 Provider 密钥、数据库内容或完整配置。

## 12. 实施顺序

1. 真实 schema 兼容与共享数据库访问边界。
2. Provider 完整读取与前端展示。
3. Provider 写入、切换和八种应用 live writer。
4. Usage 只读查询与协议能力。
5. Usage 定价、限额、会话同步和 Codex 重建。
6. 前端运行目标路由、generation 和缓存隔离。
7. 真实服务器验收、静态 Agent 依赖审计和开发环境恢复。

每一步采用 TDD：先以真实 schema 或协议行为写失败测试，再实现最小改动，通过相关回归后
独立提交。实施期间保留当前工作区已有的 SSH 完整性修复，不把无关改动混入阶段提交。

## 13. 成功标准

- 远端已有 Provider、端点和当前状态与远端 CC Switch 一致显示。
- Provider 的增删改、排序、切换和 live 配置只作用于当前目标主机。
- 使用统计页面的全部读取和写操作只作用于当前目标主机。
- 本机与各远程目标的查询缓存和迟到响应不能互相污染。
- 数据库不兼容、离线、权限、live 写入和超时均有明确错误，不静默回退本机。
- 本机模式现有行为和测试无回归。
- Agent 仍是双架构 musl 静态二进制，不依赖桌面 GUI 栈，断开后无持久安装。
