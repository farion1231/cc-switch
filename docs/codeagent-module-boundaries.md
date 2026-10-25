# CodeAgent 模块边界

目标：新增或修改 CodeAgent 行为时，优先只修改 `src/huawei/` 和
`src-tauri/src/huawei/`。独立模块降低共享业务代码的冲突机会，并不保证上游合并无冲突。

## 前端

- `src/huawei/index.ts` 注册预设、认证面板和表单扩展。
- `src/huawei/formBehavior.ts` 声明强制 Desktop 代理以及保存元数据规则。
- `src/config/formExtensions.ts` 定义通用能力和保存上下文。
- 共享表单通过 `getFormExtensionSaveMeta` 传入应用、预设目标和选中账号；
  不判断 Huawei 类型，不生成其专用字段。前端应用 ID `claude-desktop`
  在 Huawei 模块转换为后端元数据目标 `claude_desktop`。
- 选择默认账号时必须写入 `selectedAccountId: undefined`，覆盖旧账号。

## 后端

- `src-tauri/src/provider_extensions.rs` 是静态注册入口，声明纯能力接口。
  注册不依赖 Tauri setup，因此启动配置校验、命令和测试获取相同行为。
- `src-tauri/src/huawei/extension.rs` 管理类型别名、托管认证、强制 Desktop
  代理、CodeAgent 3 强制上游流式及旧 CLI 卡片的 env 默认路由。
- `claude_desktop_config.rs` 优先采用用户显式映射，仅在映射为空时询问扩展
  默认路由。托管认证可省略静态密钥，但仍要求有效端点。
- `proxy/handlers.rs` 按 `streaming_only_chat` 能力保持客户端的非流式 JSON
  契约；复用通用 SSE 解析和错误处理。普通供应商保留已有流式兜底。
- 有状态的认证、请求改写和连通性代理继续使用现有 `ProxyHook`。
  纯能力注册不承担网络请求、账号状态或生命周期管理。
- 所有 Hook 调用统一经过 `with_provider_hooks(meta, ...)`；不匹配的供应商
  即使带有同名 `extra` 字段，也不执行 CodeAgent 请求、认证、TLS 或响应策略。
- 白名单过滤整张 HeaderMap 一次，保留允许头的多值；认证在过滤之后按请求注入。
- `prefer_pooled_client` 和 `skip_tls_verify` 决定传输策略；TLS 例外必须使用
  可配置客户端。共享转发器不读取 Huawei 的 `headerMode` / `skipTlsVerify`。
- `http_client.rs` 按证书校验策略隔离连接池；TLS 例外最多缓存直连和当前代理
  两个客户端。切换代理或刷新配置使代理池失效，失败的配置更新保留旧池。
  客户端不保存账号认证头；切换账号后，每次请求重新解析认证。
- `ProviderType` 中的历史 Huawei 枚举和序列化别名保留，避免配置兼容性变化；
  元数据分类与托管上游域名策略由 Huawei 扩展提供。

## 允许的共享接入点

| 文件 | 职责 |
| --- | --- |
| `src/main.tsx` | 加载扩展注册入口 |
| `src/config/formExtensions.ts` | 通用表单扩展协议 |
| `ProviderForm.tsx` / `ClaudeDesktopProviderForm.tsx` | 调用扩展，提供保存上下文 |
| `ClaudeFormFields.tsx` | 传递认证组件和账号参数 |
| `src-tauri/src/lib.rs` | 模块声明、初始化和 Tauri 命令注册 |
| `src-tauri/src/provider_extensions.rs` | 静态组合供应商能力 |
| `claude_desktop_config.rs` | 调用扩展默认路由及认证能力 |
| `proxy/handlers.rs` | 根据能力选择响应处理策略 |
| `proxy/hooks.rs` | 按供应商匹配派发扩展 |
| `proxy/forwarder.rs` / `response_processor.rs` | 调用匹配的请求和响应 Hook |
| `proxy/http_client.rs` | 通用 TLS、代理路由和连接池策略 |
| `proxy/providers/claude.rs` | 通用托管认证和扩展分类 |
| `proxy/providers/mod.rs` | 保留类型序列化兼容，调用扩展分类 |

后续需要新行为时，先扩展协议，避免向共享文件添加供应商 ID 特判。
尚未改写全部历史定制：配置与类型兼容接入仍需在上游合并时检查；
专用旧表单的重复代码也尚未全面整合。

## 验证

- `pnpm.cmd typecheck`
- `pnpm.cmd exec vitest run tests/config/providerFormExtensions.test.ts tests/config/codeagentBoundaries.test.ts tests/components/ClaudeDesktopProviderForm.huawei.test.tsx tests/components/ClaudeDesktopProviderForm.test.tsx`
- `cargo check --lib --tests`（在 `src-tauri`）
- `cargo test --lib huawei`、`cargo test --lib extension_response_tests`、
  `cargo test --lib provider_extensions`、`cargo test --lib claude_desktop_config::tests`

模块边界测试限制核心共享流程中的 Huawei 类型字面量和直接导入；它是防止
后续回退的检查，不能替代行为测试。生成应用可执行文件仍须使用 `tauri build`。

## 本轮验证记录

TypeScript 检查、前端生产构建、Rust 库与测试编译检查通过。
Desktop 通用表单及扩展测试、36 项 Desktop 后端测试、4 项响应转换测试、
4 项代理 Hook 测试和扩展能力测试通过。

扩大检查时发现：
- OpenAI Official 预设快照多出 `providerType: codex_oauth`，使用重构前
  `ProviderForm.tsx` 可复现；本轮不更新该快照。
- 未修改的两个 connectivity_proxy TLS 测试报 `CaUsedAsEndEntity`。
  它们不属于本轮扩展边界变更，仍需另行处理证书测试夹具。

## 第二轮回归范围

- `cargo test --lib proxy::http_client -- --test-threads=1`：本机直连、Windows
  系统代理例外、TLS 连接复用、严格证书校验隔离、逐请求认证、代理切换。
- `cargo test --lib proxy::hooks`：按供应商匹配，认证 Future 不持有注册表锁。
- `cargo test --lib huawei::proxy_hooks -- --test-threads=1`：四个别名、白名单
  多值头、认证注入以及原有 CLI/Desktop 接管与恢复。
- `cargo test --lib huawei::extension`：别名分类、无静态凭证的托管认证、域名边界。
- `cargo test --lib proxy::providers::claude`、`cargo test --lib proxy::forwarder`：
  普通 Claude、Codex、Copilot 等适配器和转发回归。
- 共享代码边界检查扩展至转发、响应、Hook、HTTP 客户端与认证适配器。

第二轮上述回归共 272 项 Rust 测试通过，13 项前端扩展及边界检查通过。
本轮未重新生成应用安装包；测试使用独立 worktree 的编译产物。
