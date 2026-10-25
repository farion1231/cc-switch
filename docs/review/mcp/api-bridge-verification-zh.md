# 在线接口转 MCP 验证记录

2026-10-25，cc-switch 4.0.20，分支 `codex/mcp-api-pr`，基线为上游 `de0a235ad6389ce4afa333b472cdee615a3f3987`。本次范围仅含 API 转 MCP，不含多维用量或缓存下钻功能。

本次代码与测试 SHA-256：`ecc706ae958f23d7edbf1d1b6b3293fb583f85806b5562e39dc83e8684357611`。取相对基线变化的 `src/`、`src-tauri/`、`tests/`、`scripts/` 文件，按相对路径排序，依次拼接路径、NUL、文件内容、NUL 计算。文档不纳入该指纹。

## 验证结果

| 检查 | 实际结果 |
| --- | --- |
| `pnpm typecheck` / `pnpm format:check` | 通过 |
| `pnpm exec vitest run --maxWorkers=2 --minWorkers=1` | 256 个文件、3315 项通过 |
| API 表单专项 | 8 项通过，包含保存、错误恢复、预览失效、只读标记、重复 query、路径转义与 URL 内凭据拒绝 |
| `pnpm build:renderer` | 通过；保留现有大 chunk 提示 |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | 通过 |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | 通过 |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1` | 4285 项通过：4075 项库测试、210 项集成测试；12 项忽略，0 失败 |
| 真实 stdio 子进程 | 初始化、完整工具 Schema、静态鉴权、HTTP 调用、并发、取消、EOF 退出通过 |
| 四语言 | `zh`、`zh-TW`、`en`、`ja` 的 33 个 API 文案键一致 |

自动化测试使用独立目录，显式 `umask 022`；大小写 `NO_PROXY` 包含回环地址，仅测试子进程从 PATH 移除 Bun 全局 bin。不改变用户的系统环境。

## UI 一致性与交互

本轮使用实际 React 组件、共享样式和合成配置，在浏览器中按以下步骤检查。IPC 返回为隔离夹具，界面保存通过不等同于原生 IPC 联调通过；HTTP 和协议另用真实 Rust 子进程验证。

1. 对照原有 MCP 表单与 API 表单：页头、返回按钮、字体、输入框、勾选框、边框和固定操作栏均复用现有组件及 token，未发现明显样式偏离。
2. 检查工具预览：中文浅色、英文深色模式正常，接口 URL 可换行，服务 ID 在编辑时只读，Header 默认遮罩，目标客户端选择正常。
3. 从滚动后的表单提交错误 Header：错误提示获得焦点并回到可见区域；修正为字符串值后可完成隔离保存。未选择工具时不能保存、修改来源后预览失效，由表单回归覆盖。

截图仅用于本地 UI 检查，没有将用户本机的用量或配置截图纳入公共仓库。以上检查不构成完整无障碍合规或全部窗口尺寸兼容性证明。

## 原生保存与调用联调

macOS / arm64 的 release 安装包通过以下检查。原生测试使用本地汇总构建，其中 API 表单、MCP 面板、API 调用封装及 Rust API 运行时共 9 个文件与取消修正前的实现逐字节一致，对应代码指纹 `ed6d349f399d84a2c78aa838299da9a70c1a938b7a9f1c95de0adeab52374cbe`。本轮仅调整 stdio 的任务清理与响应顺序，原生 UI 未重测；运行时由下述真实子进程回归验证。

1. 在原生 Tauri 窗口填写合成 REST 查询接口，预览后仅选择 Codex，保存成功；界面列表、数据库和客户端配置均出现该临时服务。
2. 直接使用保存到客户端配置的 command、args 和 env 启动安装包内的 MCP 子进程，完成 initialize、tools/list 和 tools/call。Schema 未出现多余 Header 节点，查询参数及合成静态请求头均正确到达回环接口。
3. 重新打开编辑页，服务标识只读，工具和客户端选择恢复正确。提交非字符串 Header 时，表单保持打开，错误提示获得焦点且可见；修正后保存，新的请求头在实际调用中生效。
4. 从编辑页输入 OpenAPI 文档 URL，旧预览失效且保存禁用；重新预览并保存后，实际 tools/list 和 tools/call 使用新工具定义。
5. 从原生界面删除本轮临时服务后，原有供应商及 MCP 数据行哈希一致，Codex 配置解析后的整体内容与测试前一致。数据库完整性检查通过，应用启动后的日志无 ERROR。

以上仅调用回环地址上的合成 HTTP 服务，没有使用真实 API 凭据或消耗外部服务额度。此联调覆盖客户端配置的生成与执行，未验证编程客户端进程自动重载配置。

## 本次修正与失败回归

独立审查和 UI 检查发现六项问题，均在修正前实际复现失败：

- 无 Header 参数的工具经运行时处理后，多出 `headers.required: null`，导致工具 Schema 无效。改为安全读取既有节点，并检查真实 `tools/list` 的完整 Schema。
- Swagger 2.0 缺省 host / schemes 时未使用文档 URL。现在保留来源主机、IPv6、端口和协议；无来源且无法推导时明确要求基础 URL。
- Swagger 参数的排他边界和数组约束被丢弃。支持的约束保留，不支持的 `multipleOf` 明确报告跳过。
- 已知工具的输入校验错误误报为协议错误。现在返回 `isError: true`，未知工具和请求结构错误继续返回 JSON-RPC 错误，符合 [MCP 错误分类](https://modelcontextprotocol.io/specification/2025-11-25/server/tools#error-handling)。
- 手工 URL 将 `%25` 等路径编码提前解码。现在只还原路径模板括号，保留其他编码。
- 保存失败后，滚动表单中的错误提示可能在屏幕外。现在自动聚焦错误提示，重复提交也能看到反馈。

首批四项 Rust 回归及路径编码 UI 回归先失败，修复后通过；错误焦点 UI 回归同样先失败、后通过。最终全量结果见上表。

## CI 取消竞态跟进

[上游 CI 第 7361 次运行](https://github.com/farion1231/cc-switch/actions/runs/40179479474) 中，前端和 Linux 后端通过；macOS 与 Windows 后端均在 `stdio_process_bounds_concurrency_keeps_ping_responsive_and_cancels` 失败。两者都在取消响应之后仍收到并发超限错误。

原因是 `abort()` 仅安排异步任务清理，原实现立即回复取消成功，持有的并发许可可能尚未析构；正常完成路径也在响应之后才释放许可。本次保留任务 JoinHandle，取消后等待清理完成再回复；正常完成则先释放许可再输出结果。

回归不再依靠重试绕过并发拒绝，而是在收到八个取消响应后，立即提交八个新调用并逐个核对成功响应。该严格回归在修正前本地实际失败，修正后连续 20 次真实子进程运行通过。全量 `cargo test` 再次通过 4285 项、忽略 12 项；`cargo fmt --check` 和 `cargo clippy -- -D warnings` 通过。最后的类型别名调整后，两个 stdio 集成测试再次通过。远端重跑结果以 PR Checks 为准。

## 本机运行样本

以下样本来自取消修正前的代码指纹 `ed6d349f399d84a2c78aa838299da9a70c1a938b7a9f1c95de0adeab52374cbe`。macOS 26.6 / arm64，Rust debug 二进制；HTTP/1.1 回环服务返回一条合成搜索结果。运行 `python3 scripts/bench-mcp-api.py --binary <本次二进制> --samples 20`，进程启动 10 次，其余各 20 次。

| 指标 | 中位数 | P95 |
| --- | ---: | ---: |
| 新进程启动并 initialize | 7.267 ms | 7.753 ms |
| 直接 HTTP | 0.127 ms | 0.166 ms |
| stdio → HTTP 完整调用 | 0.219 ms | 0.287 ms |
| 100 工具 tools/list | 3.102 ms | 3.259 ms |

单进程 RSS 26608 KiB，约 26 MiB；100 工具发现结果 32101 字节。直接 HTTP 基线使用 Python `http.client`，桥接使用 Rust reqwest，两者差值不是纯序列化成本。该小样本只验证本机适配开销，不能外推公网延迟、QPS 或大型响应性能。

## 未验证范围

[DeepSeek 示例](../../config/mcp-api/deepseek-search-fetch.openapi.json) 的规范转换与参数约束已通过测试，但未使用真实 Key 调用，也未验证账号额度、计费或公网服务质量。本次没有验证真实编程客户端重载、Windows / Linux 打包启动。AppImage 路径选择有函数回归，Flatpak 首版明确不支持创建 API MCP。
