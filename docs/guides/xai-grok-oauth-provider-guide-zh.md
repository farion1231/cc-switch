# xAI Grok OAuth 供应商指南

> 本指南适用于 CC Switch 中面向 Claude Code / Claude Desktop 的 xAI Grok 托管 OAuth 供应商。该功能适用于拥有 Grok Build OAuth/API 权限的账号，例如符合条件的 SuperGrok 或 X Premium+ 账号。权限由 xAI 控制，登录成功不代表一定可以调用推理接口。

## 功能说明

`xAI Grok OAuth (SuperGrok / X Premium+)` 预设允许 Claude Code 或 Claude Desktop 通过 CC Switch 路由到 xAI，供应商记录中无需保存静态 xAI API Key。

默认配置：

- 供应商类型：`xai_oauth`
- Base URL：`https://api.x.ai/v1`
- 上游路径：`/v1/responses`
- API 格式：OpenAI Responses
- 默认模型：`grok-build-0.1`
- 托管认证文件：CC Switch 应用配置目录下的 `xai_oauth_auth.json`

真实 access token 只在本地代理转发请求时解析。Claude Live 配置中只写入 `PROXY_MANAGED` 占位符。

## 前置条件

- CC Switch 的本地路由服务可用。
- 已在 CC Switch 中配置 Claude Code 或 Claude Desktop。
- xAI 账号具有 Grok Build OAuth/API 权限。
- 同一台机器上有可完成回环回调的浏览器。

当前实现使用浏览器 OAuth + PKCE 和本地回环回调，不包含 device-code 流程。远程或无界面环境必须确保浏览器能够访问运行 CC Switch 的机器上的回调地址，否则请使用静态 xAI API Key 供应商。

## 添加供应商

1. 在 Claude 或 Claude Desktop 供应商表单中选择 `xAI Grok OAuth (SuperGrok / X Premium+)`。
2. 点击 xAI 登录按钮，并在浏览器中完成登录和授权。
3. 在表单中选择已登录的 xAI 账号。
4. 保存供应商。
5. 为对应应用启用本地路由并切换到该供应商。
6. 如果客户端仍加载旧配置，请重启 Claude Code 或 Claude Desktop。

保存后，供应商只记录 `authProvider = "xai_oauth"` 和所选账号 ID，不保存 bearer token。

## Live 配置与路由

- `ANTHROPIC_BASE_URL` 指向配置的 xAI 路由。
- 默认模型为 `grok-build-0.1`。
- `ANTHROPIC_API_KEY` 和非 Copilot 托管认证使用的 `ANTHROPIC_AUTH_TOKEN` 写入 `PROXY_MANAGED`。
- 真实 token 只在请求转发时从本地托管认证文件读取。

托管认证文件是应用配置目录中的 JSON 文件；Unix 平台使用仅所有者可读写的 `0600` 权限，但它不是应用层加密存储。

写入任何 Claude 托管认证供应商时，CC Switch 也会统一现有 GitHub Copilot 和 Codex OAuth 的 Live 认证环境：清除陈旧 API Key 变量并写入 `PROXY_MANAGED`，使所有托管供应商遵循同一接管契约。

xAI bearer token 只会注入到 `https://api.x.ai`。其他主机在发送前会触发托管认证保护，避免 token 泄漏。

## 存储、刷新与取消登录

账号元数据和 refresh token 保存在 `xai_oauth_auth.json`。调试输出会隐藏 access token、refresh token、ID token、授权码和 token endpoint 响应。

转发前会检查 access token；接近过期时使用 refresh token 刷新。账号不存在或已删除时，请求会在发送上游前返回托管认证错误。

浏览器登录期间会占用固定回调地址 `127.0.0.1:56121`。取消登录会立即释放监听器；启动新登录也会替换遗留监听器。如果端口被其他应用占用，CC Switch 会在打开授权页面前报告明确的端口冲突。

## 403 与权限问题

xAI 可能允许 OAuth 登录，但在推理时返回 `403`。常见原因包括订阅、API 权限、区域限制或功能灰度。

- 确认账号可在 xAI 支持的客户端中使用 Grok Build。
- 重新登录后发送小请求，排除会话过期。
- 账号没有 OAuth API 权限时，改用普通 xAI API Key 供应商。

这类错误应被视为 xAI 账号/API 权限问题，而不是供应商丢失或配置损坏。

## 安全属性

- Claude Live 配置不保存真实 xAI OAuth token。
- 发送上游前拒绝 `PROXY_MANAGED` 等占位符。
- xAI token 注入固定限制在 `https://api.x.ai`。
- 保存前要求存在已登录且可用的 xAI 托管账号。
- 取消或拒绝浏览器授权后可以立即重试。

## 手动验证清单

- 完成登录并在不填写静态 API Key 的情况下保存供应商。
- 确认 Live 配置只有 `PROXY_MANAGED`，没有真实 bearer token。
- 确认请求发送到 `https://api.x.ai/v1/responses`。
- 删除已绑定账号后，确认请求在访问上游前失败。
- 取消一次进行中的登录，确认可以立即重新登录。
- 切换离开 xAI 供应商后，确认其他 Claude 供应商仍然存在。

## 参考资料

- [xAI Grok Build 0.1 公告](https://x.ai/news/grok-build-0-1)
- [Hermes Agent xAI Grok OAuth 指南](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/guides/xai-grok-oauth.md)
- [OpenClaw xAI 文档](https://docs.openclaw.ai/providers/xai)
- [OpenCode 供应商文档](https://opencode.ai/docs/providers/)
- [CC Switch xAI Grok OAuth 实现契约](../research/xai-grok-oauth-contract.md)
