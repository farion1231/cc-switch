# 在线接口转 MCP

## 目标与调研

让已有 HTTP API 通过 MCP 被编程客户端调用，提供 OpenAPI/Swagger 文档导入和单个 REST 接口配置两种入口。导入只生成配置，业务请求在客户端调用工具时发生。

2026-10-25 检索上游 Issue、PR 的 OpenAPI、Swagger、REST MCP、接口转换关键词，未找到同类转换实现；检索不能证明不存在。[#7953](https://github.com/farion1231/cc-switch/issues/7953) 是第三方 HTTP MCP 模板，[#7795](https://github.com/farion1231/cc-switch/issues/7795) 与 [#8466](https://github.com/farion1231/cc-switch/pull/8466) 是管理 cc-switch 的 REST 控制台，均不解决普通 API 转 MCP。

现有 `McpService` 保存标准 MCP server 配置并同步到各客户端，没有 API 转换运行时。实现继续复用该存储与同步路径。

## 方案比较

| 方案 | 收益 | 成本 |
| --- | --- | --- |
| 外部 OpenAPI 转换器 | 可复用更广的协议支持 | 依赖额外运行环境、包安装和版本管理 |
| 内置 Rust stdio 适配器（采用） | 随应用交付、无额外运行环境、脱离窗口运行 | 需明确支持的 OpenAPI 子集并维护协议测试 |
| 常驻 HTTP 网关 | 多客户端共享连接与缓存 | 增加监听端口、鉴权、生命周期与常驻资源成本 |

## 用户流程

MCP 页新增“从 API 创建”。文档入口接受 URL 或粘贴的 JSON/YAML；手工入口填写工具名、方法、完整 URL、说明及参数定义。两种入口均展示编译后的工具清单，默认只选 GET/HEAD，写接口需主动勾选。最后填写服务标识、静态请求 Header 并选择目标客户端，再保存。

编辑已创建的服务可调整工具选择、Header 和目标客户端。原始文档不会自动刷新；重新导入后再确认工具清单，避免上游规范变化自动扩大能力。

只读性质独立于 HTTP 方法。手工接口可以标注“只读业务查询”，文档导入支持操作级 `x-mcp-read-only`，通过 MCP annotations 告知客户端；未标注时保守按方法推断。POST 搜索接口因此可标为只读，但仍需主动勾选启用，且只读不代表免费调用。

## 数据与运行

- 每个服务是标准 stdio MCP：`command` 为本机 cc-switch 可执行文件，`args` 为 `--mcp-api`。
- 版本化、规范化的工具定义压缩后放入 `CC_SWITCH_MCP_API_CONFIG`；Header 单独放入 `CC_SWITCH_MCP_API_HEADERS`。两者随现有 server 配置保存在 SQLite 并投影到客户端。凭据仍为明文配置，不宣称加密存储。
- 不生成第二份配置文件，不打开数据库，不启动 Tauri 窗口或单实例插件。运行入口先于 GUI 初始化。导出的命令绑定本机安装位置；移动安装位置后需重新创建服务配置。
- 工具定义最多 100 个，解压后最多 512 KiB，编码定义与静态 Header 合计最多 24 KiB。超出时要求缩小接口集合，降低平台环境变量限制和工具上下文膨胀的影响。
- stdio 使用逐行 JSON-RPC，提供 initialize、initialized、ping、tools/list、tools/call 和调用取消。并发上限 8；超限立即返回错误，不无限排队。客户端关闭 stdin 后结束子进程。
- 每个进程复用一个 reqwest Client 和连接池；不缓存业务响应，不重试写请求。连接超时 5 秒，总超时 30 秒，文档下载上限 5 MiB，业务响应上限 2 MiB。

## 支持范围与失败语义

支持 OpenAPI 3.0/3.1、Swagger 2.0 的 JSON/YAML，内部 `$ref`、常见 path/query/header 参数和 application/json 请求体。工具参数以 `path`、`query`、`headers`、`body` 分组，避免同名冲突。常用标量、数组和 JSON 对象保留 schema；复杂或不能忠实映射的参数序列化、循环/外部引用、文件与表单、服务器变量等明确拒绝对应接口，预览列出原因。

允许用户明确指定公网、内网和本机 API；拒绝非 HTTP(S)、URL 内凭据、云元数据地址，禁用重定向，防止静态 Header 被带到另一个地址。URL 参数只进入预定路径或查询，不能修改目标 origin。API 的静态 Header 优先于模型参数，Host、Content-Length、Connection 等传输 Header 不允许自定义。

静态 Header 按大小写不敏感匹配，可满足文档中对应的必填 Header；工具定义中不回显其值，也不会创建原本不存在的 Header Schema。请求体 Schema 投影时排除 `readOnly` 字段及其必填要求。Swagger 缺省 host / schemes 时取文档 URL 的主机、端口和协议；粘贴文档且无法推导时要求提供基础 URL。已支持的参数约束保留并规范化，不能支持的验证关键字明确跳过。AppImage 启动命令使用持久的原文件路径，Flatpak 首版明确不支持创建 API MCP。

导入只对文档做 GET，不探测业务接口。不从文档执行代码，不拉取外部引用。不自动完成 OAuth2、签名认证、浏览器登录、分页遍历、上传或业务 SSE。认证不足由上游错误显式呈现。未知工具和请求结构错误返回 JSON-RPC 参数错误；已知工具的输入校验、网络和 HTTP 错误返回 `isError: true` 的工具结果，便于模型获得错误后修正参数。网络错误不回显带查询参数的完整 URL。错误分类遵循 [MCP 工具规范](https://modelcontextprotocol.io/specification/2025-11-25/server/tools#error-handling)。

## 性能与验收

转换复杂度约为被遍历的文档与 schema 大小；每次工具调用仅增加参数映射、JSON 编解码和本机 stdio 传递。网络通常主导总延迟，但必须以本机实测为依据。工具数量和 schema 体积会增加模型上下文成本，因此选择接口优于整份规范无筛选暴露。

验收覆盖：两入口生成可编辑 MCP；规范解析、内部引用、跳过原因、路径转义、query/JSON/Header 映射、错误与大小限制；真实 stdio 初始化、工具发现和本机 HTTP 调用；并发和取消；原有 MCP 回归、类型检查、格式、Rust 编译与相关测试。性能记录区分文档转换、进程冷启动、直接 HTTP 与 stdio 桥接调用，报告环境、样本与局限，不能据本机回环测试承诺公网 QPS。
