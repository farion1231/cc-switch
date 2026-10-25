# 在线接口转 MCP 验证记录

2026-10-25，cc-switch 4.0.19，分支 `codex/mcp-api-bridge`，基线 `ceb41445e998c5642e93800c9d9d836c5f73cb97` 之上的工作区变更。

核心实现 SHA-256：`9f826217d9970270f766ba05d02f9574e49f7c4e2248d15d08f8eb991c5b0b64`。范围为 `src-tauri/src/mcp_api/*.rs`、MCP commands / lib / main、API 编辑组件 / MCP 面板 / 前端 API 包装及性能脚本，按相对路径排序拼接路径、NUL、文件内容、NUL 计算。

## 结果与边界

双入口、配置保存、冻结规范、参数映射和独立 stdio 子进程已实现。DeepSeek [Search / Fetch 官方接口](https://api-docs.deepseek.com/guides/search/) 已有 [可导入示例](../../config/mcp-api/deepseek-search-fetch.openapi.json) 和 [配置说明](../../config/mcp-api/usage-zh.md)。未使用真实 API Key 调用 DeepSeek，也未验证其账号额度、计费或公网服务质量。

| 检查 | 实际结果 |
| --- | --- |
| 前端全套 `pnpm test:unit` | 261 个文件、3307 项通过；其后增加只读标注，最终表单专项 6 项通过 |
| Rust 全套 `cargo test` | 命令级环境隔离后，19 个测试二进制合计 4272 项通过、13 项忽略 |
| 最终 API Rust 专项 `cargo test --lib mcp_api_` | 15 项通过、1 个性能用例默认忽略 |
| 最终真实进程 `cargo test --test mcp_api_stdio` | 2 项通过 |
| `pnpm typecheck` / `pnpm format:check` | 通过 |
| `pnpm build:renderer` | 通过；保留现有大 chunk 提示 |
| `cargo fmt --check` / `cargo clippy -- -D warnings` | 通过 |
| 扩展检查 `cargo clippy --all-targets -- -D warnings` | 受基线 `database/tests.rs:1563,1567` 的 `comparison_to_empty` 阻断；对应行已核对存在于基线，未扩大修改范围 |
| 可视检查 | 浏览器中的实际 React 组件，使用合成服务配置；确认工具清单、只读标注、Header 遮罩、折叠重导入区及固定保存栏 |

首轮 Rust 全套出现两项环境相关失败：`omp_search_paths_include_the_bun_global_bin` 假设 PATH 不含 Bun，而当前 shell 已包含；`describe_reqwest_error_keeps_cause_and_drops_url` 的回环请求经过代理得到 502，未按预期连接拒绝。复测只在命令环境中移除 Bun PATH 项、给大小写 `NO_PROXY` 增加 localhost / 127.0.0.1 / ::1，并使用工作区 `.tmp/mcp-api-test-home` 隔离配置；未修改这两项测试或相关产品代码。

全套运行后最终调整限于 API 模块和表单的只读标注，已重跑对应专项、真实进程、类型、格式和构建检查。未将此前全套结果冒充这些最终差异的全套重跑结果。

## 可复现性能测量

环境为 macOS 26.6 / arm64，Rust debug 二进制。HTTP 返回单条合成搜索结果，使用回环 HTTP/1.1 和持久连接。请求样本各 200 次，进程启动 10 次，100 工具发现 20 次。

```sh
python3 scripts/bench-mcp-api.py --binary src-tauri/target/debug/cc-switch --samples 200
CC_SWITCH_TEST_HOME="$PWD/.tmp/mcp-api-test-home" cargo test --manifest-path src-tauri/Cargo.toml --lib mcp_api_benchmark_conversion -- --ignored --nocapture
```

| 指标 | 中位数 | P95 |
| --- | ---: | ---: |
| 新进程启动并 initialize | 7.07 ms | 7.68 ms |
| 直接 HTTP | 0.085 ms | 0.122 ms |
| stdio → HTTP 完整调用 | 0.195 ms | 0.260 ms |
| 100 工具 tools/list | 3.86 ms | 4.36 ms |

单个进程 RSS 为 26720 KiB，约 26.1 MiB。100 个合成工具的发现结果为 34901 字节。直接 HTTP 基线使用 Python `http.client`，桥接使用 Rust reqwest，两者差值不是纯粹的序列化成本；新进程启动也不代表冷文件缓存。数据仅用于判断本地适配成本，不能外推公网 QPS、实际搜索延迟或大型响应场景。

文档转换各运行 10 次：

| 接口数 | 文档字节 | 平均转换耗时 |
| ---: | ---: | ---: |
| 1 | 186 | 0.98 ms |
| 25 | 2880 | 2.54 ms |
| 100 | 11355 | 6.65 ms |
| 1000 | 114855 | 63.69 ms |

当前实现适合一般业务接口。批量调用应保持进程复用；每个客户端、每个服务均会启动自己的进程，因此服务数量较多时须计入约 26 MiB/进程的本机样本成本。更多工具也会增加模型上下文负担，建议按业务用途筛选和拆分。

## 已覆盖的失败模式

回归覆盖重复 query 丢失、路径注入、静态鉴权覆盖与必填 Header、OpenAPI 3.0 排他边界、请求只读字段、外部/循环引用、压缩炸弹、重定向、过大响应、HTTP 错误、并发超限和取消。数组唯一性校验改用规范化 JSON 加哈希去重；10 万唯一整数校验约 54 ms，避免平方级扫描占满运行线程。

AppImage 持久路径选择有纯函数测试，Windows/Linux 打包运行未实际验证；Flatpak 首版明确不支持创建 API MCP。未执行真实编程客户端重载、原生 Tauri 全流程或真实 DeepSeek 联调。协议验证使用真实 cc-switch 子进程和合成 HTTP 服务，不等同于所有客户端版本的兼容性证明。
