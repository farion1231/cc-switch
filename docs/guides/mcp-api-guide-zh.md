# 将业务 API 配置成 MCP

在 MCP 页点击“从 API 创建”，可以导入 OpenAPI / Swagger 文档 URL、粘贴文档，或选择“REST 接口”填写单个接口。预览后选择需要的工具，填写服务标识和静态请求头，勾选目标客户端后保存。客户端重载 MCP 配置后即可发现并调用工具，无需编写转换脚本。配置会保存接口定义快照；规范变化后需重新导入确认。

## DeepSeek 搜索示例

DeepSeek 官方提供独立的 [Search / Fetch API](https://api-docs.deepseek.com/guides/search/)，可供任何支持 MCP 工具的模型使用。

1. 打开 [接口定义示例](mcp-api-deepseek-search-fetch.openapi.json)，将 JSON 正文粘贴到“或粘贴 JSON / YAML 文档”。
2. 点击“预览工具”，主动勾选 `deepseek_search`、`deepseek_fetch`。两者使用 POST，因此不会默认勾选。
3. 服务标识填写 `deepseek-web`，静态请求头填写下面的 JSON，将占位符替换为自己的 Key。
4. 选择客户端并保存。请求头由适配器注入，Key 无需作为模型调用参数。

```json
{"Authorization": "Bearer YOUR_DEEPSEEK_API_KEY"}
```

模型调用 `deepseek_search` 的参数示例：

```json
{"body": {"query": "Rust MCP site:github.com", "count": 5}}
```

调用 `deepseek_fetch`：

```json
{"body": {"url": "https://example.com/article"}}
```

这些请求直接发往 DeepSeek，使用其账号额度与计费规则；实际费用以官方文档为准。示例不包含凭据，不会在导入时发送搜索请求。

## 单个 REST 接口

填写工具名、HTTP 方法和完整 URL。URL 中的 `{id}` 会生成必填路径参数，查询串会转换为带默认值的 query 参数，重复查询参数会保留为数组。其他参数用 OpenAPI 参数数组配置，例如：

```json
[{"in":"query","name":"keyword","required":true,"schema":{"type":"string"}}]
```

需要 JSON 请求体时填写 Schema，例如 `{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}`。模型调用时参数按 `path`、`query`、`headers`、`body` 分组。接口的静态认证建议使用独立的“静态 API 请求头”字段，避免把密钥写进参数默认值或工具说明。

POST 也可能是只读查询，例如搜索。手工配置可勾选“只读业务查询”；OpenAPI 操作可增加 `"x-mcp-read-only": true`。预览会展示该性质，客户端会收到对应的只读提示。它不改变业务权限，也不代表调用免费。

## 运行范围

适配器随 cc-switch 交付，由客户端按需启动；窗口关闭不影响已经配置的服务。移动应用安装位置或在另一台机器恢复配置后，应重新保存 API 服务以刷新本机启动路径。AppImage 使用原始文件路径；Flatpak 首版暂不支持创建 API MCP。

每个服务最多 100 个工具，定义与静态 Header 的编码总量最多 24 KiB；超出时拆成多个服务。最多并发 8 个调用，单次请求超时 30 秒、响应最多 2 MiB；不自动重试业务请求。静态 Header 在 cc-switch 与客户端配置中以明文保存。

首版支持常见 path/query/header 参数和 JSON 请求体；不支持 OAuth 登录自动化、文件上传、表单和流式业务响应。不支持的操作会列在预览的跳过原因中。HTTP 错误作为工具错误返回，工具的语义与访问权限仍由原业务 API 决定。

Swagger 2.0 文档未写 `host` 或 `schemes` 时，会使用文档 URL 的主机、端口或协议。粘贴文档没有来源 URL，需在文档中补齐这些字段，或填写“覆盖 API 基础地址”。
