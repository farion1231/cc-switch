# API Key 变量刷新脚本模板

这是一个完整、无需 npm 依赖的 Node.js 20+ 刷新脚本。`index.mjs` 会导入配置和辅助模块，因此可以把供应商对接代码放在独立文件中维护。

## 离线测试

1. 解压 ZIP，在 **设置 → API Key 变量 → 脚本目录** 选择解压后的 `api-key-refresh-template` 目录。
2. 在该目录运行 `node index.mjs`。默认 `demo` 模式会输出一个 JSON 对象，Key 明确以 `demo-not-a-real-key-` 开头。
3. 在 CC Switch 添加 `MY_KEY`，选择此目录后点击 **测试脚本**。可在供应商配置中用 `{{MY_KEY}}` 引用保存后的值。

演示 Key 不能访问真实 API，只用于验证脚本契约和多文件导入。

## 支持的 API Key 字段

在支持的 API Key 字段中将 `{{MY_KEY}}` 作为完整值填写即可。CC Switch 会为 Claude、Claude Desktop、Codex、Gemini、Grok Build、OpenCode、OpenClaw、Hermes、Pi、MiniMax Code 和统一供应商解析它；原生配置编辑器中的模型级 `apiKey`、`api_key` 字段也包含在内。不会在 `Bearer` 字符串、任意请求头、MCP/工具凭据、用量设置或 OAuth 凭据中插值。

## 配置 HTTP 供应商

1. 编辑 `config.mjs`，或在启动 CC Switch 前设置 `API_KEY_TEMPLATE_MODE=http` 和 `API_KEY_REFRESH_URL=https://your-provider.example/refresh`。默认的 `example.invalid` 地址会被刻意拒绝。
2. 按供应商接口修改 `config.mjs` 中的 POST JSON 请求体，以及 `lib/provider.mjs` 中的“响应映射位置”。
3. 在启动 CC Switch **之前**，将 `API_KEY_REFRESH_TOKEN` 设置到启动应用的环境中。不要把 token 写进模板目录，也不要输出到日志。环境变更后重启 CC Switch。
4. 先执行 `node index.mjs`，再在页面点击 **测试脚本**。

HTTP 示例会发出 POST JSON，并使用 `Authorization: Bearer $API_KEY_REFRESH_TOKEN`。它期望响应形如 `{ "apiKey": "...", "expiresAt"?: "...", "refreshAfter"?: "...", "message"?: "..." }`；请在 `lib/provider.mjs` 将实际响应转换成此结构。

PowerShell（从同一个终端启动 CC Switch）：

```powershell
$env:API_KEY_TEMPLATE_MODE = "http"
$env:API_KEY_REFRESH_URL = "https://provider.example/v1/refresh"
$env:API_KEY_REFRESH_TOKEN = "替换为真实密钥"
node index.mjs
# 手动测试成功后，从此终端启动或重启 CC Switch。
```

POSIX shell：

```sh
export API_KEY_TEMPLATE_MODE=http
export API_KEY_REFRESH_URL=https://provider.example/v1/refresh
export API_KEY_REFRESH_TOKEN='替换为真实密钥'
node index.mjs
```

手动成功输出只有一行，例如：

```json
{
  "apiKey": "provider-key-without-spaces",
  "expiresAt": "2030-01-01T00:00:00.000Z",
  "refreshAfter": 1893454200,
  "message": "refreshed"
}
```

`expiresAt` 和 `refreshAfter` 可使用带时区的 RFC 3339 / ISO-8601 字符串（例如 `2030-01-01T00:00:00Z`）或 Unix **秒**。不要使用 Unix 毫秒；`expiresAt` 必须在未来。

## 运行契约

CC Switch 在所选目录执行 `index.mjs`，需要 Node 20+；自动发现失败时可在 **脚本运行环境** 设置 Node 可执行文件路径。成功时 stdout 只能输出一个不超过 64 KiB 的 JSON 对象。诊断信息写入 stderr，且不得记录凭据、Key、认证头或原始供应商响应。

CC Switch 会提供：

- `CC_SWITCH_CONTRACT_VERSION=2`
- `CC_SWITCH_VAR_NAME`：正在刷新的变量名
- `CC_SWITCH_REASON`：`startup`、`scheduled`、`switch`、`manual`、`test` 或 `retry`
- `CC_SWITCH_CURRENT_EXPIRES_AT`：可选，当前过期时间的 ISO-8601 UTC 字符串

配置错误、认证失败和无效响应应以退出码 `2` 结束，让用户处理；网络错误、超时、HTTP 408/429/5xx 应以退出码 `1` 结束，让 CC Switch 按退避策略重试。模板已按此规则实现。

脚本默认超时为 30 秒（页面可设为 5 到 300 秒），默认在过期前 5 分钟刷新。已启用的脚本会在启动、计划刷新时运行；当前 Key 即将过期时，切换前也可能运行。调度优先使用 `refreshAfter`，否则使用 `expiresAt` 减去提前刷新时间；两者都没有时，成功后每小时运行一次。测试失败时，先确认所选目录包含 `index.mjs`，在同目录手动运行相同命令，检查 **脚本运行环境** 中的 Node 路径，再查看 stderr；不要为了排查而把密钥写入日志。
