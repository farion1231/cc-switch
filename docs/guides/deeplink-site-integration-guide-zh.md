# 在你的网站上放「一键导入 CC Switch」按钮：站长接入指南

中文 | [English](deeplink-site-integration-guide-en.md) | [日本語](deeplink-site-integration-guide-ja.md)

如果你是站长，可以通过 CC Switch 让你的用户一键把你的供应商导入到 Claude Code、Codex 等 Agent 应用中使用。

> 适用版本：CC Switch v4.0.10 及以上。文中标了「v4.0.10 起」的行为在更早的版本上不同，差异见文末「兼容旧版本」。本文根据当前代码整理，示例里的域名和 Key 都是假的。

## 这篇写给谁

你运营一个 API 服务（中转站、聚合平台、自建网关都算），想让用户在你的控制台点一下按钮，就把你的地址、Key、模型，以及余额查询，一起加进 CC Switch，不用再手动填表。

这件事靠 `ccswitch://` 深链接完成。用户的系统装了 CC Switch 之后，浏览器点到 `ccswitch://...` 链接会唤起 CC Switch，弹出导入确认框，用户核对后点「导入」。

用户手册里的 [深度链接协议](../user-manual/zh/5-faq/5.3-deeplink.md) 是参数速查表；这篇讲站长真正要做的决定：链接怎么生成、哪些东西能带、哪些会被拒绝、怎么让用户导入后直接看到余额。

## 最快路径

> 你的站是用 New API 或 Sub2API 搭的？两者的 Key 列表里都自带「导入到 CC Switch」按钮，开箱即用，不用自己开发。区别在余额：New API 的链接不带用量查询脚本，用户导入后卡片上看不到余额；Sub2API 的链接带。想补上余额、带上你的图标，或者支持更多应用，再按这篇改。

大多数站长只要做三件事，按顺序读下面几节就够了：

1. 读「先看结论」，再读「供应商链接」里的「最小的一条」「支持哪些应用」「常用参数」，定下给哪些应用生成链接、地址和格式怎么写。可以先在 [在线生成器](https://ccswitch.io/deplink.html) 里填表单生成一条，在自己电脑上点一下，看看确认框长什么样。
2. 想让用户导入后看到余额，读「用量查询脚本」。NewAPI / OneAPI、Sub2API 系的站有现成脚本，照抄即可。
3. 读「生成链接的代码」和「把链接放到按钮上」：后端按用户生成链接，前端放按钮。

折叠起来的几节（附带配置原文、会被拒绝的链接汇总、MCP / 提示词 / Skill）用到时再展开。

## 先看结论

1. **链接要按用户生成**。导入供应商时 API Key 是必填的，没有 Key 的链接会导入失败。所以「一键导入」按钮应该放在用户登录后的控制台里，由你的后端用这个用户自己的 Key 拼出链接，不要放在公开页面上。按钮怎么做见「把链接放到按钮上」。
2. **用 `apiFormat` 声明你的接口格式**。取值 `anthropic`、`openai_chat`、`openai_responses`、`gemini_native`，各应用支持哪几种见下文。不写时按应用的默认格式：Claude Code 是 Anthropic Messages，Codex 和 Grok Build 是 OpenAI Responses，OpenCode / OpenClaw / Hermes / Pi 是 OpenAI Chat Completions。
3. **模型名按需写**。中转站提供的是原版 Claude 模型时，Claude Code 的模型全部留空，Claude Code 会用自带的模型列表；只有 DeepSeek、Kimi 这类非原生模型才需要把 `model`、`haikuModel`、`sonnetModel`、`opusModel` 写上。OpenCode、OpenClaw、Hermes 的模型列表来自 `model`，不写就没有可选的模型；Pi 必须写。
4. **想让用户看到余额，就带上用量查询脚本**。v4.0.10 起，带了 `usageScript` 就默认启用；想兼容旧版本，同时写上 `usageEnabled=true`。脚本只能请求与供应商地址同源的 HTTPS 接口，详见下文。
5. **「导入后立即切换」由用户决定**。确认框里有这个开关，链接里的 `enabled` 只是开关的初始值。一般不用写。
6. **Base64 参数要再做 URL 转义**。用 `URLSearchParams` 或等价函数拼链接，不要手工拼字符串。

## 链接的结构

```
ccswitch://v1/import?resource=<类型>&<参数>...
```

- `v1` 是协议版本，`/import` 是固定路径，写错都会被拒绝。
- `resource` 取 `provider`（供应商）、`mcp`、`prompt`（提示词）、`skill` 四者之一。
- 参数名区分大小写，用驼峰：`apiKey`、`usageScript`，不是 `api_key`。

### 编码规则

有两层编码，顺序不能反：

1. `config`、`usageScript`、`content` 这类「整段内容」的参数，先把 UTF-8 文本做 **Base64** 编码。标准 Base64 和 URL-safe Base64 都认，末尾的 `=` 可有可无。
2. 拼进链接时，**所有参数值**再做一次 URL 转义（`application/x-www-form-urlencoded` 规则）。标准 Base64 里的 `+` 必须转成 `%2B`，否则会被解成空格。CC Switch 会尝试把空格还原成 `+`，但别依赖它。

`URLSearchParams` 会自动完成第 2 步。手写链接时最常见的错误就是漏了这一步。

布尔参数写 `true` / `false`，其他写法会被当成没写。

## 用户点了链接之后

1. 浏览器询问「是否打开 CC Switch」，用户同意后 CC Switch 被唤起（没开着的话会先启动）。
2. CC Switch 弹出导入确认框，展示链接里的全部内容：地址、打码后的 Key、模型、附带的配置原文、用量查询脚本全文。可疑的内容会标黄提示，例如指向本机或内网的地址、能改变进程加载行为的环境变量（`NODE_OPTIONS`、`LD_PRELOAD` 之类）、用 shell 执行整段命令的 MCP。
3. 用户点「导入」后：
   - **供应商**：加进对应应用的供应商列表。确认框底部有一个开关，初始值取链接里的 `enabled`，用户可以改：
     - Claude Code、Codex、Gemini CLI、Antigravity CLI、Grok Build：开关是「导入后立即切换到这个供应商」，默认关闭。打开时确认框会提示「客户端请求会改发到上面的地址」。
     - OpenCode、OpenClaw、Hermes、Pi：这些应用可以同时放多个供应商，开关是「导入后立即添加到应用配置」（Pi 叫「启用」），默认打开；关掉则只保存在 CC Switch 的列表里。
   - **MCP**：立即写进 `apps` 里列出的每个应用的配置文件。
   - **提示词**：加进提示词列表。确认框里有「导入后立即启用」开关，初始值取链接里的 `enabled`；打开时会替换这个应用当前的提示词文件，原来启用的提示词被停用。
   - **Skill**：添加的是 Skill 仓库，用户还要到「Skills」页选择安装哪些。

几个站长容易踩到的细节：

- 链接只能由用户点击触发。页面加载时自动跳转 `ccswitch://` 会被大多数浏览器拦下。
- CC Switch 一次只处理一条链接；用户还没处理完上一条时又点了新的，只保留最新那条。不要设计成「一个按钮连续触发多条链接」，供应商、MCP、提示词各给一个按钮，让用户依次点。
- 同一条供应商链接点两次会导入两份。供应商 ID 由名称加时间戳生成，不做去重。
- 网页无法可靠地判断用户是否装了 CC Switch。按钮旁边写一句提示并附上下载地址，写法见「把链接放到按钮上」。

## 供应商链接

### 最小的一条

```
ccswitch://v1/import?resource=provider&app=claude&name=Example%20AI&endpoint=https%3A%2F%2Fapi.example.com&apiKey=sk-xxxx
```

必填的只有 `app`、`name`，以及地址和 Key（可以写在 URL 参数里，也可以写在附带的 `config` 里）。Pi 另外必须写 `model`。

### 支持哪些应用

| `app` | 应用 | 默认格式 | 可选的 `apiFormat` | `config` 能带什么 |
|-------|------|---------|--------------------|-------------------|
| `claude` | Claude Code | `anthropic` | 四种都可以 | `{"env": {...}}`，`env` 里的变量全部保留 |
| `codex` | Codex | `openai_responses` | `openai_responses`、`openai_chat`、`anthropic` | 整份或部分 `config.toml`，见下文 |
| `gemini` | Gemini CLI | `gemini_native` | 只有 `gemini_native` | `GEMINI_API_KEY`、`GOOGLE_GEMINI_BASE_URL`、`GEMINI_MODEL` 三个键，其他忽略 |
| `agy` | Antigravity CLI | `gemini_native` | 只有 `gemini_native` | 扁平 Gemini 环境键或 `{env, config:{model:{name}}}`；URL 参数优先，模型优先取 `config.model.name`；其他原生选项忽略 |
| `grokbuild` | Grok Build | `openai_responses` | `openai_responses`、`openai_chat`、`anthropic` | 一张 `[model."<名称>"]` 表，见下文 |
| `opencode` | OpenCode | `openai_chat` | 四种都可以 | 只取地址和 Key |
| `openclaw` | OpenClaw | `openai_chat` | 四种都可以 | 只取地址和 Key |
| `hermes` | Hermes | `openai_chat` | `openai_chat`、`openai_responses`、`anthropic` | 只取地址和 Key |
| `pi` | Pi | `openai_chat` | 四种都可以 | 只取地址和 Key |

Claude Desktop、MiniMax Code 的供应商暂不支持深链接导入，链接会被拒绝。

`apiFormat` 在不同应用里落到不同的地方：

- **Claude Code、Codex、Grok Build**：这三个客户端只会说自己的原生格式（Claude Code 是 Anthropic，另两个是 Responses）。写了非原生格式，导入的卡片会带「需要路由」标记，用户切过去时要开启本地路由，由 CC Switch 转换请求格式。
- **OpenCode、OpenClaw、Hermes、Pi**：这些客户端自己支持多种格式，CC Switch 把 `apiFormat` 换算成它们配置里的协议字段（OpenCode 的 `npm`、OpenClaw / Pi 的 `api`、Hermes 的 `api_mode`），客户端直接用这种格式请求你的接口，不经过本地路由。

地址按所选格式的惯例写：Anthropic 格式在 Claude Code 里不带 `/v1`（如 `https://api.example.com`），OpenAI 两种格式一般带 `/v1`。OpenCode、OpenClaw、Hermes、Pi 的地址是各自 SDK 的 base URL，拼路径的方式不统一：同样是 Anthropic 格式，CC Switch 内置预设里 OpenCode 的地址带 `/v1`，Hermes 和 Pi 的不带。请为每个应用分别生成链接，地址以你在该应用里实测能通的写法为准。

### 常用参数

| 参数 | 说明 |
|------|------|
| `name` | 显示名，必填 |
| `endpoint` | 接口地址。可以用英文逗号分隔多个，第一个是主地址，其余加进「端点候选」供用户测速切换 |
| `apiKey` | API Key。URL 参数和 `config` 里都有时，以 URL 参数为准 |
| `apiFormat` | 接口格式，见上表。不写用应用的默认格式 |
| `model` | 默认模型。Claude Code 用原版 Claude 模型时留空；Codex 不写会用 `gpt-5-codex`，Grok Build 不写会用 `grok-4.5`；Pi 必填 |
| `haikuModel` / `sonnetModel` / `opusModel` | 仅 Claude Code：三档模型别名对应的真实模型名，非原生模型才需要 |
| `homepage` | 官网。不写时从地址推断（去掉 `api.` 前缀） |
| `icon` | CC Switch 内置图标的名字，只能是小写字母、数字和 `. _ -`，其他写法会被丢掉；不认识的名字显示名称首字母。你的品牌没有内置图标时不用写 |
| `iconUrl` | 你自己的图标图片地址，见下文「带上你的图标」。下载失败时退回 `icon` |
| `notes` | 备注，显示在供应商卡片上 |
| `enabled` | 确认框里「导入后立即切换 / 添加」开关的初始值，用户可以改。一般不用写 |
| `config` + `configFormat` | 附带配置原文，Base64。`configFormat` 取 `json`（默认）或 `toml` |

`configUrl`（远程配置）还没实现，带了这个参数的链接会被直接拒绝。

### 带上你的图标：iconUrl

CC Switch 内置图标里没有你的品牌时，用 `iconUrl` 给一个图片地址。确认框一打开，CC Switch 就下载这张图并显示在顶部，用户导入后它就是卡片图标，不需要用户再点什么。

图片会被重新画成 96×96 的 PNG 存进 CC Switch，之后不再访问这个地址。你以后换了图标，已经导入的用户不会跟着变。

地址和图片要满足下面的条件，任何一条不满足，确认框不报错，图标退回 `icon` 指定的内置图标，没写 `icon` 就显示名称首字母：

- `https://` 开头，主机是域名（不能是 IP），默认端口，不带账号密码。形状不对的地址在解析链接时就被丢掉，链接其余部分照常导入。
- 公开可访问：CC Switch 下载时不带 Cookie 和 Referer，也不走用户的系统代理。需要登录、开了防盗链的地址都会失败。
- 域名解析到的地址必须是公网地址，指向内网、本机的一律不下载。
- 5 秒内下载完，最多跟 3 次重定向，每次重定向的目标也要满足上面这些条件。
- 不超过 256 KB。
- 响应头 `Content-Type` 是 `image/png`、`image/jpeg`、`image/webp` 或 `image/svg+xml` 之一，并且和文件实际格式一致。`application/octet-stream` 之类的通用类型不行。
- 位图宽高不超过 4096 像素。

图片本身的建议：

- 用方形图。四角不透明的方图会铺满整个图标框；四角透明的 logo 会缩到中间，四周留白，和内置图标的样子一致。
- SVG 只当图片显示，里面的脚本不执行，引用的外部字体、图片也不加载，文字请转成路径。
- 每个用户打开确认框时都会请求一次这个地址，放在 CDN 上即可。

### Claude Code：附带环境变量

<details>
<summary>需要额外环境变量（自定义请求头、超时等）时展开</summary>

需要额外环境变量的站（自定义请求头、超时、关闭实验特性等），把它们放进 `config`：

```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "https://api.example.com",
    "ANTHROPIC_AUTH_TOKEN": "sk-xxxx",
    "ANTHROPIC_MODEL": "example-sonnet",
    "API_TIMEOUT_MS": "600000",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1"
  }
}
```

`env` 里的变量原样保留；地址、Key、模型名这几个，URL 参数里也写了的话以 URL 参数为准。确认框会把每个变量列出来，敏感值打码。

</details>

### Codex：附带 config.toml

<details>
<summary>只有要带 <code>wire_api</code>、请求头、推理强度等额外设置时才需要，展开看收哪些键、拒绝哪些写法</summary>

两种写法：

- `configFormat=json`，内容是 `{"auth": {"OPENAI_API_KEY": "sk-xxxx"}, "config": "<config.toml 原文>"}`；
- `configFormat=toml`，内容直接是 `config.toml` 原文。

CC Switch 不会把整份文件照搬进用户的 `~/.codex/config.toml`，只收属于这个供应商的部分：

- **选中的那张 `[model_providers.<id>]` 表**：`model_provider` 指向的那张；没写 `model_provider` 时文件里只能有一张。表里收这些键：`name`、`base_url`、`wire_api`、`requires_openai_auth`、`experimental_bearer_token`、`model_catalog_url`、`capabilities`、`query_params`、`http_headers`、`request_max_retries`、`stream_max_retries`、`stream_idle_timeout_ms`、`websocket_connect_timeout_ms`、`supports_websockets`、`supports_standalone_web_search`。
- **顶层的模型相关字段**：`model`、`review_model`、`model_reasoning_effort`、`plan_mode_reasoning_effort`、`disable_response_storage`、`web_search`、`model_context_window`、`model_auto_compact_token_limit`、`model_supports_reasoning_summaries`、`model_verbosity`，以及 `[agents]` 的子代理模型、`[memories]` 的记忆模型。

其他的一律不收：`[features]`、`[tools]`、MCP、sandbox、`notify`、其他供应商表等，这些是用户自己的全局设置。

导入后表名统一改成 `custom`（CC Switch 管理的 Codex 供应商都写在 `[model_providers.custom]`），`base_url` 和 `experimental_bearer_token` 用合并后的地址和 Key。`model_catalog_url` 必须与供应商地址同源，否则拒绝导入。

**会被拒绝的写法**：表里带 `auth`（让 Codex 执行命令取 Key）、`aws`、`gateway_oauth`（读本机云凭据）、`env_http_headers`（读环境变量）。`env_key` 会被丢掉，Key 一定要由链接自己给出；只写了 `env_key`、没给 Key 的链接会被拒绝。

示例（`configFormat=toml`）：

```toml
model_provider = "example"
model = "example-coder"
model_reasoning_effort = "high"

[model_providers.example]
name = "Example AI"
base_url = "https://api.example.com/v1"
wire_api = "responses"
http_headers = { "X-Client" = "cc-switch" }
stream_idle_timeout_ms = 300000
```

</details>

### Grok Build：附带一张模型表

<details>
<summary>只有要带上下文窗口、采样参数等额外设置时才需要</summary>

写法同 Codex（`{"config": "<config.toml 原文>"}` 或 `configFormat=toml`）。

只导入 `[models] default` 指向的那张 `[model."<名称>"]` 表；没写 `default` 时文件里只能有一张表，有多张又没写 `default` 会被拒绝。CC Switch 里一个 Grok Build 供应商只对应一张模型表，所以一条链接导入多个模型不支持。

表里保留显示名、`description`、`api_backend`、`context_window`、`max_completion_tokens`、采样和重试参数、推理强度相关字段等；地址、Key、模型名用链接参数（URL 参数优先）。表里没写 `api_backend`、`context_window` 时补默认值。

表里带 `auth_provider`、`env_http_headers`、`mtls_cert_dir` 的链接会被拒绝；`env_key` 处理同 Codex。

</details>

### 会被拒绝的链接，汇总

<details>
<summary>确认框报错时展开，按报错原文查原因</summary>

| 情况 | 确认框里的报错（英文原文，节选） |
|------|-------------------------------|
| 没给 Key | `API key is required` |
| 没给地址 | `Endpoint is required` |
| 地址不是 http / https | `Invalid URL scheme for 'endpoint[0]'` |
| `app` 写成了不支持的值 | `Invalid provider app type` |
| `apiFormat` 取值不对 | `Unsupported apiFormat` |
| 应用不支持这种 `apiFormat` | `is not supported for app` |
| Pi 没写模型 | `Pi providers need a model` |
| OpenCode / OpenClaw / Hermes / Pi 的名称、地址、Key、模型里带了会被客户端展开的写法（见下） | `uses syntax that … would expand` |
| 带了 `configUrl` | `'configUrl' is not supported yet` |
| Base64 解不开 | `config 参数 Base64 解码失败` |
| Codex / Grok 只靠 `env_key` 给 Key | `supplies its API key indirectly` |
| Codex 表里有 `auth` 等 | `cannot be imported from a link` |
| Codex 目录地址与供应商地址不同源 | `model_catalog_url` |
| Grok 有多张表但没写 `default` | `several [model.*] tables` |

OpenCode、OpenClaw、Hermes、Pi 读配置时会展开某些写法：Pi 的 Key 以 `!` 开头会被当成命令执行，`$NAME` / `${NAME}` 换成环境变量；OpenCode 的 `{env:…}` 读环境变量、`{file:…}` 读本机文件；OpenClaw、Hermes 的 `${…}` 读环境变量。从链接导入时，这些写法一律拒绝。你的 Key 和地址本来就是字面值，不受影响。

</details>

## 让用户导入后直接看到余额：用量查询脚本

CC Switch 的供应商卡片可以显示余额或套餐用量，数据来自一段用量查询脚本。你把脚本放进链接，用户导入后就能直接看到。

### 脚本长什么样

脚本是一段 JavaScript 表达式，返回一个对象，包含 `request`（发什么请求）和 `extractor`（怎么从响应里取数）：

```javascript
({
  request: {
    url: "{{baseUrl}}/user/balance",
    method: "GET",
    headers: {
      "Authorization": "Bearer {{apiKey}}"
    }
  },
  extractor: function (response) {
    return {
      remaining: response.balance,
      unit: "USD"
    };
  }
})
```

运行前，CC Switch 会把脚本里的占位符替换成实际值：

| 占位符 | 替换成 |
|--------|--------|
| `{{apiKey}}` | `usageApiKey`；没给时用供应商的 API Key |
| `{{baseUrl}}` | `usageBaseUrl`；没给时用供应商的主地址（去掉末尾 `/`） |
| `{{accessToken}}` | `usageAccessToken` |
| `{{userId}}` | `usageUserId` |

注意 `{{baseUrl}}` 是供应商地址原样，如果它带了 `/v1`，拼出来就是 `https://api.example.com/v1/user/balance`。余额接口不在 `/v1` 下时，直接在脚本里写完整的 URL 即可，只要和地址同源。

`extractor` 收到的是解析后的响应 JSON，返回一个对象或对象数组（多个套餐时），字段都可选：

| 字段 | 类型 | 说明 |
|------|------|------|
| `remaining` | 数字 | 剩余额度 |
| `used` | 数字 | 已用 |
| `total` | 数字 | 总额度 |
| `unit` | 字符串 | 单位，如 `USD`、`CNY`、`次` |
| `planName` | 字符串 | 套餐名 |
| `isValid` | 布尔 | Key 是否有效；`false` 时卡片显示失效 |
| `invalidMessage` | 字符串 | 失效原因 |
| `extra` | 字符串 | 附加说明 |

### 运行限制

脚本是不可信输入，CC Switch 对它有硬限制：

- 跑在 QuickJS 沙箱里，没有 `fetch`、文件、网络等任何能力；脚本本身只负责「描述请求」和「解析响应」，请求由 CC Switch 代发。
- **每次查询只发一个请求**，不能先登录再查询。
- **请求必须是 HTTPS，且与 `{{baseUrl}}` 同源**（同域名、同端口）。不同源的请求会被拒绝。余额接口在另一个域名时（比如控制台在 `console.example.com`，API 在 `api.example.com`），用 `usageBaseUrl` 指向控制台域名。
- 响应必须是 JSON。
- 请求由 CC Switch 从用户电脑上直接发出，不经过浏览器，你的余额接口不需要配置 CORS。接口要能用 API Key 认证；只能用登录态或系统访问令牌查询时，用 `usageAccessToken` 把令牌带进链接（见下面 NewAPI 一节）。
- 脚本执行时间上限 5 秒，内存上限 16 MiB；HTTP 请求超时 10 秒。

### 启用与否

| 链接写法 | 导入后 |
|----------|--------|
| 带 `usageScript`，不写 `usageEnabled` | 启用（v4.0.10 起；更早的版本不启用） |
| 带 `usageScript`，`usageEnabled=true` | 启用 |
| 带 `usageScript`，`usageEnabled=false` | 不启用，用户可以在应用里手动打开 |
| 不带脚本 | 不启用 |

确认框会完整显示脚本正文和「已启用 / 未启用」标记。

`usageAutoInterval` 设自动查询间隔（分钟），`0` 或不写表示不定时查询。别设得太短，每个用户都会按这个频率打你的接口。

### NewAPI / OneAPI 系的站

这类站的余额接口用的是系统访问令牌和用户 ID，不是 API Key。链接里带上 `usageAccessToken` 和 `usageUserId`：

```javascript
({
  request: {
    url: "{{baseUrl}}/api/user/self",
    method: "GET",
    headers: {
      "Authorization": "Bearer {{accessToken}}",
      "New-Api-User": "{{userId}}"
    }
  },
  extractor: function (response) {
    if (response.success && response.data) {
      return {
        planName: response.data.group,
        remaining: response.data.quota / 500000,
        used: response.data.used_quota / 500000,
        unit: "USD"
      };
    }
    return { isValid: false, invalidMessage: response.message };
  }
})
```

`500000` 是 NewAPI 默认的额度换算比例，你改过的话按自己的值写。系统访问令牌的权限比 API Key 大，只在用户本人登录后生成的链接里放。

### Sub2API 系的站

Sub2API 自带给 CC Switch 用的 `GET /v1/usage` 接口，用 API Key 认证，不需要 `usageAccessToken`。返回的顶层字段就是 CC Switch 认的 `isValid`、`planName`、`remaining`、`unit`，脚本基本照搬：

```javascript
({
  request: {
    url: "{{baseUrl}}".replace(/\/+$/, "").replace(/\/v1$/, "") + "/v1/usage",
    method: "GET",
    headers: {
      "Authorization": "Bearer {{apiKey}}",
      "User-Agent": "cc-switch/1.0"
    }
  },
  extractor: function (response) {
    var quota = response.quota || {};
    var result = {
      isValid: response.isValid !== false,
      unit: response.unit || quota.unit || "USD"
    };
    if (typeof response.planName === "string") result.planName = response.planName;
    if (typeof response.remaining === "number" && response.remaining >= 0) {
      result.remaining = response.remaining;
    }
    if (quota.limit > 0) {
      result.used = quota.used;
      result.total = quota.limit;
    }
    if (!result.isValid) {
      result.invalidMessage = response.status
        ? String(response.status)
        : "查询失败";
    }
    return result;
  },
})
```

- Codex、Grok Build 的供应商地址带 `/v1`，Claude Code 的不带。脚本先去掉末尾的 `/v1` 再拼接，所以各个应用通用，不用写 `usageBaseUrl`。
- 三种情况都有 `remaining`：钱包余额（`planName` 是「钱包余额」）、订阅分组（`planName` 是分组名，取日 / 周 / 月限额里剩得最少的那个）、Key 单独设了总额度（另有 `quota.used` 和 `quota.limit`）。订阅分组没设任何限额时 `remaining` 是 `-1`，表示不限量，脚本不显示这个数。
- 响应里还有 `rate_limits`（Key 的 5 小时 / 1 天 / 7 天窗口）和 `subscription`（分组的日 / 周 / 月用量和限额）。想在卡片上分行显示，可以让 `extractor` 返回数组，每个元素一行。
- 这份脚本和 CC Switch 应用内「配置用量查询」的 Sub2API 模板（v4.0.10 起）完全一样，用户手动添加你的站时也能一键选用。
- Sub2API 后台「导入到 CC Switch」按钮生成的链接自带一份查询脚本，用了 `?.` 和 `??` 写法，CC Switch 也能运行，但订阅分组不限量时会显示 `-1`。

## MCP、提示词、Skill 链接

<details>
<summary>和 API 服务关系不大，顺带分发配套工具时展开</summary>

这三类和你的 API 服务关系不大，但有的站会顺带分发配套工具，简单列一下要点。

**MCP**（`resource=mcp`）

- `apps` 必填，逗号分隔：`claude`、`codex`、`gemini`、`agy`、`grokbuild`、`opencode`、`hermes`、`pi`、`mcode`。`openclaw` 会被忽略。
- `config` 是 Base64 的 `{"mcpServers": {"<ID>": {...}}}`，一条链接可以带多个服务器。
- 用户已经有同 ID 的服务器时，只会给它勾上新的应用，配置内容不会被链接覆盖。想更新配置请换一个 ID。
- 导入即写入客户端配置文件。`enabled` 参数目前不起作用。

**提示词**（`resource=prompt`）

- `app` 取 `claude`、`codex`、`gemini`、`agy`、`grokbuild`、`opencode`、`pi`、`mcode`。`openclaw`、`hermes` 在 CC Switch 里由工作区编辑器和记忆管理负责，不建议用深链接分发提示词。
- `name`、`content`（Base64 的 Markdown）必填，`description` 可选。
- `enabled` 是确认框里「导入后立即启用」开关的初始值。启用会用这段内容替换用户当前的提示词文件（如 `CLAUDE.md`），一般不要写。

**Skill**（`resource=skill`）

- `repo` 必填，格式 `owner/name`，必须是 GitHub 上的公开仓库；`branch` 默认 `main`。
- 添加的是仓库，用户还要自己选择安装哪些 Skill。`directory` 参数目前会被忽略。

</details>

## 生成链接的代码

### 后端（Node.js）

```javascript
function b64(text) {
  return Buffer.from(text, "utf8").toString("base64");
}

const usageScript = `({
  request: {
    url: "{{baseUrl}}/user/balance",
    method: "GET",
    headers: { "Authorization": "Bearer {{apiKey}}" }
  },
  extractor: function (r) {
    return { remaining: r.balance, unit: "USD" };
  }
})`;

function claudeImportLink(user) {
  const params = new URLSearchParams({
    resource: "provider",
    app: "claude",
    name: "Example AI",
    endpoint: "https://api.example.com",
    apiKey: user.apiKey,
    // 提供原版 Claude 模型时模型全部留空；DeepSeek、Kimi 这类非原生模型才写
    // model / haikuModel / sonnetModel / opusModel
    homepage: "https://example.com",
    // 内置图标里没有你的品牌时，给一个自己的图标地址，见「带上你的图标」
    iconUrl: "https://example.com/static/logo.png",
    usageScript: b64(usageScript),
    usageEnabled: "true",
  });
  return `ccswitch://v1/import?${params}`;
}
```

给 OpenCode 生成一条 Anthropic 格式的链接，只是换 `app` 和 `apiFormat`，再带上模型：

```javascript
const params = new URLSearchParams({
  resource: "provider",
  app: "opencode",
  name: "Example AI",
  endpoint: "https://api.example.com/v1",
  apiKey: user.apiKey,
  apiFormat: "anthropic",
  model: "claude-sonnet-5-5",
});
```

### 后端（Python）

```python
import base64
from urllib.parse import urlencode

def b64(text: str) -> str:
    return base64.b64encode(text.encode("utf-8")).decode("ascii")

codex_toml = """\
model_provider = "example"
model = "example-coder"

[model_providers.example]
name = "Example AI"
base_url = "https://api.example.com/v1"
wire_api = "responses"
"""

def codex_import_link(api_key: str) -> str:
    params = {
        "resource": "provider",
        "app": "codex",
        "name": "Example AI",
        "apiKey": api_key,
        "configFormat": "toml",
        "config": b64(codex_toml),
    }
    return "ccswitch://v1/import?" + urlencode(params)
```

`urlencode` 默认把空格转成 `+`、把 `+` 转成 `%2B`，和 CC Switch 的解析规则一致。

### 浏览器里生成

浏览器的 `btoa` 不支持中文等非 ASCII 字符，要先转 UTF-8：

```javascript
function b64(text) {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  bytes.forEach((b) => (binary += String.fromCharCode(b)));
  return btoa(binary);
}
```

能在后端生成就在后端生成，Key 不经过前端 JavaScript 更好。

## 把链接放到按钮上

推荐的流程：

1. 按钮本身不带链接。用户点击时，前端向你的后端请求这个用户的链接。
2. 后端确认用户已登录，用这个用户自己的 Key 生成链接返回（用上一节的生成函数）。
3. 前端拿到链接后执行 `window.location.href = link`。浏览器询问是否打开 CC Switch，用户同意后弹出导入确认框。

这样链接不会写进页面源码，只在用户真的点了按钮时生成一次。

### 后端：按用户返回链接

以 Node.js（Express）为例。`claudeImportLink` 是上一节的生成函数，`codexImportLink` 照同样的写法另写一个：

```javascript
const builders = {
  claude: claudeImportLink,
  codex: codexImportLink,
};

app.post("/api/ccswitch-link", requireLogin, (req, res) => {
  const build = builders[req.body.app];
  if (!build) return res.status(400).json({ error: "unsupported app" });
  // 响应里有用户的 Key：不缓存，也别打进日志
  res.set("Cache-Control", "no-store");
  res.json({ link: build(req.user) });
});
```

用 POST，不用 GET：GET 请求可能被浏览器预取、被缓存，或者带着参数出现在访问日志里。

### 前端：按钮和提示

```html
<button type="button" onclick="importToCcSwitch('claude')">导入到 Claude Code</button>
<button type="button" onclick="importToCcSwitch('codex')">导入到 Codex</button>
<p class="hint">
  浏览器会先询问是否打开 CC Switch，请选「打开」。点了没有任何反应？可能还没安装，
  先<a href="https://ccswitch.io" target="_blank" rel="noopener">下载 CC Switch</a>，装好后再点一次。
</p>

<script>
  async function importToCcSwitch(app) {
    const res = await fetch("/api/ccswitch-link", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ app }),
    });
    if (!res.ok) {
      alert("生成导入链接失败，请稍后再试");
      return;
    }
    const { link } = await res.json();
    window.location.href = link;
  }
</script>
```

几点说明：

- **跳转要紧跟着点击**。浏览器只放行用户点击触发的外部应用唤起，有的浏览器在点击后隔得太久就不再算作点击触发。接口应在一两秒内返回；不要在拿到链接后再弹一个确认框，让用户再点一次。
- **一条链接只导入到一个应用**。同时支持几个应用时，每个应用放一个按钮，或者先让用户选择客户端，再点同一个「导入」按钮。每个应用的地址和格式可能不同，见「支持哪些应用」。
- **没装 CC Switch 时浏览器通常什么都不提示**，网页也无法可靠地判断。所以按钮旁边的那句提示要一直显示，不要等检测结果。
- 同一个按钮被点两次会导入两份。用户点完一次后，可以把按钮改成「已发送到 CC Switch」，或者暂时禁用几秒。

## 安全与隐私

- **带 Key 的链接等同于 Key 本身**。直接写进按钮 `href` 的链接会出现在页面源码里，可能被浏览器插件读到；按「把链接放到按钮上」的做法，在用户点击时才向后端要链接，可以避免这一点。只在用户登录后的页面生成，不要写进公开页面、邮件群发或社群公告；不要让它进入你的访问日志和第三方统计。
- 链接越短越好。整份 `config` 很长时，Windows 下传给 CC Switch 的命令行有约 3.2 万字符的上限，超长的链接打不开。
- CC Switch 对链接内容做的是白名单过滤和醒目提示，不是替用户背书。确认框显示的每一项用户都能看到，别在里面放和你的服务无关的东西，用户会因此不信任这个按钮。

## 调试

1. 先在自己电脑上点一遍，看确认框里的每一项是否符合预期，再导入、切换、发一条消息。
2. 确认框直接报错时，错误信息就是原因，对照上面「会被拒绝的链接」排查。
3. 导入成功但余额不显示：在供应商卡片上打开「配置用量查询」，点「测试脚本」看报什么错。最常见的是请求地址不同源、余额接口返回的不是 JSON、`extractor` 里取错了字段。
4. 导入成功但切过去后请求失败：多半是接口格式不对（见「支持哪些应用」），或模型名不对。
5. `iconUrl` 的图标没显示：下载失败不会报错，原因写在 CC Switch 的日志里（`~/.cc-switch/logs/cc-switch.log`，搜 `Skipped deep link icon`）。最常见的是 `Content-Type` 不对、图片超过 256 KB、地址要登录或开了防盗链，用 `curl -I` 看一下响应头。

## 兼容旧版本

你的用户不一定都升级到了最新版。下面这些行为在旧版本上不同：

| 行为 | v4.0.10 起 | v3.19.0 – v4.0.9 |
|------|-----------|------------------|
| 带脚本、不写 `usageEnabled` | 启用 | 不启用 |
| Codex 附带的 `config.toml` | 按白名单收下供应商表和模型字段 | 只取地址、Key 和模型名，其余丢弃，按固定模板重建 |
| Grok Build 附带的模型表 | 收下选中表的字段 | 只取地址、Key 和模型名 |
| CC Switch 没开着时点链接 | 启动后弹出确认框 | 偶尔丢失，要再点一次 |
| `apiFormat` | 按应用换算 | 忽略，一律用默认格式 |
| `app=pi` | 支持 | 拒绝导入 |
| `iconUrl` | 下载并作为卡片图标 | 忽略，显示 `icon` 或名称首字母 |
| 「导入后立即切换 / 启用」 | 确认框里的开关，`enabled` 是初始值 | 直接按链接的 `enabled` 执行；OpenCode / OpenClaw / Hermes 总是写进应用配置 |

想让各个版本表现一致：用量查询显式写 `usageEnabled=true`；Codex / Grok Build 的关键信息（地址、Key、模型）同时写进 URL 参数，不要只放在 `config` 里。你的接口不是应用的默认格式时，旧版本导入的卡片格式会不对，可以在按钮旁提示用户升级 CC Switch。
