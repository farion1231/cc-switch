# Issue #7482 修复计划与工作记录

> 本文档是可持续更新的工作清单。开始一项工作后，将对应复选框改为 `[x]`，并同步更新「状态总览」和文末「变更记录」。

## 基本信息

| 项目 | 内容 |
| --- | --- |
| Issue | [#7482 Local proxy compares its gateway auth token in non-constant time (CWE-208), reachable over non-loopback bind](https://github.com/farion1231/cc-switch/issues/7482) |
| 当前状态 | 修复已实现，待合并验证 |
| 核实日期 | 2026-09-18 |
| 核实提交 | `06082e189d65e6d6dbadc35dacdac1ce6c79d89a`（`main`，2026-09-15） |
| 仓库版本 | `cc-switch` / `src-tauri` 3.20.3 |
| 严重度判断 | Low；存在确定的计时侧信道缺陷，但远程稳定利用受网络噪声、样本量和非 loopback 配置限制 |
| 责任人 | 待分配 |
| 目标版本 | 待维护者确认 |
| 修复 PR | [#7488](https://github.com/farion1231/cc-switch/pull/7488) |

## 结论摘要

**属实。**

当前源码在 Claude Desktop gateway 的 Bearer token 校验中使用普通字符串比较：

```rust
if token != expected {
    return Err(ProxyError::AuthError(
        "Claude Desktop gateway token 无效".to_string(),
    ));
}
```

`expected` 是持久化的 `ccs-<UUID v4 simple>` 随机 token，`token` 来自请求的 `Authorization` 头。Rust 的字符串相等比较通常会在首个不同字节处提前返回，因此比较耗时可能随正确前缀长度变化，构成 CWE-208/CWE-385 类型的计时侧信道。

该认证路径保护以下两个实际注册的 Claude Desktop 路由：

- `GET /claude-desktop/v1/models`
- `POST /claude-desktop/v1/messages`

代理监听地址来自用户配置；`SECURITY.md` 明确把非 loopback 监听时来自其他主机的入站请求列入威胁范围。因此，在用户把代理绑定到非 loopback 接口的条件下，局域网攻击者属于有效攻击面。

## 已核实事实

- [x] 确认认证函数位置：`src-tauri/src/proxy/handlers.rs:271-296`。
- [x] 确认第 290 行使用 `token != expected`，不是常量时间比较。
- [x] 确认 token 生成逻辑：`src-tauri/src/claude_desktop_config.rs:278-289`，格式为 `ccs-{uuid::Uuid::new_v4().simple()}`。
- [x] 确认两个受保护路由均调用 `validate_claude_desktop_gateway_auth`：
  - `handle_claude_desktop_messages`：`src-tauri/src/proxy/handlers.rs:134-148`
  - `handle_claude_desktop_models`：`src-tauri/src/proxy/handlers.rs:150-164`
- [x] 确认路由注册位置：`src-tauri/src/proxy/server.rs:299-307`。
- [x] 确认监听地址可配置，服务使用配置值执行 `TcpListener::bind`：`src-tauri/src/proxy/server.rs:100-114`。
- [x] 确认 `SECURITY.md:20-22` 和 `SECURITY.md:72-73` 明确覆盖非 loopback 监听下的外部入站请求。
- [x] 确认 `/status` 当前无认证且返回 `current_provider`、`current_provider_id`、`last_error` 等状态字段：`src-tauri/src/proxy/handlers.rs:72-76`、`src-tauri/src/proxy/types.rs:58-92`。
- [x] 确认仓库当前已有 `hmac = "0.12"` 依赖，`Cargo.lock` 中也存在 `subtle`/`constant_time_eq` 的传递依赖；但这不等于可以直接依赖传递 crate，修复时应显式声明所选 API。

## 修复目标

- [x] 对 Claude Desktop gateway token 使用常量时间比较。
- [x] 不改变合法请求的成功行为，不改变现有 token 的生成、存储和配置流程。
- [x] 不扩大 token、Authorization 头或秘密内容进入日志/错误响应的范围。
- [x] 增加能锁定正确/错误/缺失/畸形认证行为的回归测试。
- [x] 评估并处理认证失败错误文本差异，避免额外的可区分信号。
- [x] 评估 `/status` 的未认证信息暴露；保持现有本地 UI/CLI 行为兼容，记录接受理由。
- [x] 形成可审查、可回滚、可验证的最小变更。

## 修复计划清单

### 1. 复现与基线

- [x] 阅读 `CONTRIBUTING.md`、测试脚本和当前 CI 配置，确认 Rust 测试入口。
- [x] 在依赖可用的环境执行认证相关 Rust 测试基线，记录命令与结果。
- [x] 为认证校验提取可测试的纯函数边界，避免测试必须启动完整代理服务器。
- [x] 记录当前正确 token、错误 token、缺失头、非 UTF-8 头、大小写 Bearer、空白处理行为。
- [x] 若条件允许，在受控本机/容器环境做高样本计时基线；该实验只用于确认风险，不作为修复正确性的唯一验收。

**验收证据**

- [x] 基线测试命令及结果已记录。
- [x] 当前行为矩阵已记录。
- [x] 计时实验（如执行）已记录环境、样本量、统计方法和限制。

### 2. 实现常量时间比较

- [x] 选择实现方案：
  - 首选：显式添加 `subtle` 依赖并使用 `ConstantTimeEq::ct_eq`。
  - 备选：复用已显式声明的 `hmac`，对两侧做 HMAC/摘要后比较。
- [x] 如果选择 `subtle`，在 `src-tauri/Cargo.toml` 显式加入直接依赖；不要依赖 `Cargo.lock` 中的传递依赖。
- [x] 在 `validate_claude_desktop_gateway_auth` 中对 token 的 UTF-8 字节执行常量时间比较。
- [x] 对长度不同的输入保持安全失败；不要因长度短路提前返回。
- [x] 保留现有 Bearer 解析语义，或明确记录并测试有意调整的语义。
- [x] 不记录 token、expected token 或完整 Authorization 头。

**建议改动点**

- `src-tauri/Cargo.toml`
- `src-tauri/src/proxy/handlers.rs:271-296`

**验收证据**

- [x] diff 中不再存在该认证路径的普通 `==`/`!=` 秘密比较。
- [x] `cargo fmt --check` 通过。
- [x] `cargo clippy`（按仓库实际启用范围）通过，或记录既有失败与本改动无关。

### 3. 统一认证失败响应

- [x] 决定是否把「缺失 Authorization」「格式无效」「token 无效」统一为同一个外部 401 错误文本/响应。
- [x] 若保持不同文本，说明为何这些差异不会形成可利用的 oracle，并补充测试锁定决定。
- [x] 确保日志中不新增 token 片段或可逆信息。
- [x] 检查 `ProxyError::AuthError` 到 HTTP 401 的映射保持稳定：`src-tauri/src/proxy/error_mapper.rs`、`src-tauri/src/proxy/error.rs`。

**验收证据**

- [x] 四类失败输入的状态码和响应体行为已测试。
- [x] 不存在 token 值进入响应或日志的断言/代码审查结论。

### 4. 回归测试

- [x] 正确 token 成功。
- [x] 错误 token 失败。
- [x] 仅前缀正确的 token 失败。
- [x] 正确 token 但长度多/少一个字节失败。
- [x] 缺失 `Authorization` 头失败。
- [x] 非 UTF-8 `Authorization` 头失败。
- [x] `Bearer ` 与 `bearer ` 大小写兼容行为符合现有约定。
- [x] 两侧空白的行为符合现有约定。
- [x] 测试使用固定测试 token，不使用生产数据库或真实用户 token。
- [x] 若提取纯函数，测试直接覆盖纯函数；另保留至少一个路由级/认证入口级测试。

**验收证据**

- [x] 新增测试全部通过。
- [x] 失败用例返回 401，成功用例不被误拒绝。
- [x] 测试名称清晰表达计时安全修复的意图。

### 5. `/status` 低风险加固评估

> 该项是 issue 中标注的 informational/minor 项，不应扩大主修复范围，但应明确处理决定。

**决定：保持现状，不在本 PR 中修改。**

- 桌面 UI 通过 Tauri `get_proxy_status` 读取同一结构，`active_targets` 还用于当前供应商展示；给 HTTP `/status` 单独加认证或改结构会造成不必要的行为分叉。
- `current_provider`、`current_provider_id` 和 `last_error` 只会在代理监听地址已经暴露给不可信网络时被远端读取；这属于用户主动将本地代理绑定到非 loopback 的既有风险，应通过监听地址告警和后续独立设计处理，而不是在计时侧信道修复中改变 API。
- `last_error` 确实可能包含上游 URL、供应商上下文或错误文本，因此该项保留为后续加固候选；触发条件是维护者决定支持非 loopback 多主机部署，或用户报告状态端点信息暴露。

- [x] 确认 `/status` 是否需要被本机 UI、CLI、测试或外部集成访问。
- [x] 评估以下方案：
  - 保持现状并记录接受理由。
  - 仅在 loopback 上暴露详细状态。
  - 对 `/status` 增加认证。
  - 把敏感字段从公开响应中移出，另设本地 IPC 接口。
- [x] 检查 `last_error` 是否可能包含供应商名称、URL、路径或其他敏感上下文。
- [x] 选择方案后补测试并更新威胁模型/文档（如行为改变）。

**验收证据**

- [x] 有明确的接受/修复决定。
- [x] 若修复，兼容性影响已评估。
- [x] 若接受，记录风险、暴露前提和后续触发条件。

### 6. 文档与发布

- [x] 在变更说明中记录：CWE-208/CWE-385、受影响端点、非 loopback 前提、修复版本。
- [x] 如项目维护者认为符合条件，评估 GitHub Security Advisory/CVE 流程。
- [x] 检查是否需要更新 `SECURITY.md` 或用户手册中关于代理监听地址的警告。
- [x] 检查 release notes 是否需要安全条目，避免披露可直接利用的细节。
- [x] 确认回滚方式：单独回滚常量时间比较改动即可恢复旧行为，不涉及数据迁移。

**验收证据**

- [x] 文档/发布条目已准备。
- [x] 回滚步骤已记录。

### 7. 合并前最终验证

- [x] `git diff --check` 通过。
- [x] Rust 格式检查通过。
- [x] Rust 单元/集成测试通过，或完整记录无法执行的环境原因。
- [x] 前端检查仅在改动触及前端时执行；否则记录为不适用。
- [x] 审查最终 diff，确认没有无关重构、没有秘密泄漏、没有行为回归。
- [x] 在 issue 中回复核实结果、修复 PR/提交和验证证据。

**验收证据**

- [x] 最终命令清单及结果已记录。
- [x] issue/PR 链接已记录。

## 建议实现草案

以下只是计划草案，尚未提交为实现代码：

```rust
use subtle::ConstantTimeEq;

let token_bytes = token.as_bytes();
let expected_bytes = expected.as_bytes();

if token_bytes.ct_eq(expected_bytes).unwrap_u8() != 1 {
    return Err(ProxyError::AuthError(
        "Claude Desktop gateway token 无效".to_string(),
    ));
}
```

实现时需注意：

- `ct_eq` 对长度不同会安全返回 false，但应在测试中覆盖。
- `token` 经过 `.trim()` 后仍可能包含非 ASCII 字符；按字节比较保持确定性。
- 不要把 token 长度、匹配前缀长度或比较结果写入日志。
- 若仓库选择 `hmac` 方案，应明确 HMAC key 的生命周期和常量时间验证方式。

## 状态总览

| 阶段 | 状态 | 更新日期 | 备注 |
| --- | --- | --- | --- |
| Issue 核实 | 已完成 | 2026-09-18 | 源码、路由、威胁模型均吻合；结论为属实 |
| 复现与基线 | 已完成 | 2026-09-18 | 已确认测试入口并建立行为矩阵 |
| 常量时间比较实现 | 已完成 | 2026-09-18 | 使用显式 `subtle` 依赖与 `ConstantTimeEq` |
| 认证错误统一 | 已完成 | 2026-09-18 | 缺失、畸形、错误 token 统一为固定 401 文案 |
| 回归测试 | 已完成 | 2026-09-18 | 覆盖成功、失败、畸形、大小写和空白边界 |
| `/status` 加固评估 | 已完成 | 2026-09-18 | 接受现状以保持 UI/CLI 兼容；记录非 loopback 暴露前提 |
| 文档与发布 | 已完成 | 2026-09-18 | 修复说明、验证证据和回滚方式已记录 |
| 合并前验证 | 已完成 | 2026-09-18 | 本机验证完成；PR [#7488](https://github.com/farion1231/cc-switch/pull/7488) 已创建 |

状态取值建议：`未开始`、`进行中`、`已阻塞`、`待审查`、`已完成`、`不适用`。

## 风险与注意事项

- 这是安全敏感改动，但不应借机重构整个代理认证层。
- 常量时间比较只能消除比较本身的时序差异；代理服务器、网络、TLS、日志和错误路径仍可能有其他可观测差异。
- 远程计时攻击需要足够样本和稳定网络；无法在当前环境稳定复现，不代表源码缺陷不成立。
- 如果代理只绑定 `127.0.0.1`，远程攻击面显著缩小，但本机其他进程仍可能访问；修复仍应完成。
- 不应在测试、文档、日志或提交信息中写入真实 gateway token。

## 验证命令记录

> 每执行一次验证，在下方追加一条记录；不要覆盖旧记录。

| 日期 | 环境 | 命令 | 结果 | 备注 |
| --- | --- | --- | --- | --- |
| 2026-09-18 | Windows / Git checkout | `rg -n ...` 静态源码核验 | 已确认 | 已核对函数、路由、监听配置、威胁模型 |
| 2026-09-18 | Windows / Rust 1.95.0 | `cargo test --manifest-path src-tauri/Cargo.toml --lib claude_desktop_gateway_auth -- --nocapture` | 通过 | 5 个认证专项测试通过 |
| 2026-09-18 | Windows / Rust 1.95.0 | `cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::handlers::tests -- --nocapture` | 通过 | 55 个 handler 测试通过 |
| 2026-09-18 | Windows / Rust 1.95.0 | `cargo test --manifest-path src-tauri/Cargo.toml --lib` | 部分通过 | 2861 通过、10 忽略；2 个既有符号链接测试因 Windows 1314 无权限失败 |
| 2026-09-18 | Windows / baseline `06082e1` | 两个符号链接测试的基线复现 | 已确认 | detached worktree 中同样以 Windows 1314 失败，与本次改动无关 |
| 2026-09-18 | Windows / Rust 1.95.0 | `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | 通过 | 无格式差异 |
| 2026-09-18 | Windows / Rust 1.95.0 | `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings` | 通过 | 无 Clippy 警告 |
| 2026-09-18 | Windows / Rust 1.95.0 | `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | 既有失败 | `transform_codex_chat.rs:4328` 的 `clippy::op_ref` 与本次改动无关 |
| 2026-09-18 | Windows / Rust 1.95.0 | `cargo test --manifest-path src-tauri/Cargo.toml` | 环境失败 | 集成测试目标缺少 `cc_switch_lib`/Tauri rlib；仓库 CI 与本地可用入口为 `--lib` |
| 2026-09-18 | 本次改动范围 | 前端检查 | 不适用 | 未修改 `src/**` 或前端依赖 |

## 变更记录

| 日期 | 状态 | 更新内容 | 更新人 |
| --- | --- | --- | --- |
| 2026-09-18 | 已核实，待修复 | 创建计划；完成 issue 与源码交叉核验；标记修复阶段 | Codex |
| 2026-09-18 | 修复已实现 | 常量时间比较、统一 401、认证回归测试与 `/status` 评估完成；已创建 PR #7488 | Codex |
