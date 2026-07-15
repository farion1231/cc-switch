# 在 Codex 中用 Claude：CC Switch 本地路由攻略

> 适用版本：CC Switch 3.17.0 及以上（「Anthropic Messages 上游」自 3.17.0 引入）。本文根据仓库内文档与代码整理，以 Claude 系中转网关为例演示。截图来自当前前端界面，使用去敏示例数据生成，避免泄露真实 API Key。

## 为什么需要本地路由

新版 Codex CLI 面向的是 OpenAI Responses API，而各类 Claude 系中转网关、企业内部网关暴露的是 Anthropic Messages 协议，也就是 `/v1/messages`。这两种协议的请求体、流式事件和返回结构完全不同，把这类网关的接口地址直接填进 Codex 配置里，结果只能是请求 `/responses` 返回 404。

这个功能面向的就是「手里只有 `/v1/messages` 端点」的场景：你有某个 Claude 系中转网关的 Key，想用 Codex 的交互习惯跑 Claude 系列模型；或者公司出于合规策略禁用了 Claude Code 客户端、只保留了经批准的 Claude 系网关——模型本身可用，缺的只是一个被允许的客户端，现在 Codex 可以补上这个位置。

CC Switch 的做法是让 Codex 始终连本机路由，仍以 Responses API 发送请求；路由识别当前供应商是 Anthropic 格式后，把请求转换成 Anthropic Messages 发给上游，再把响应转换回 Responses 形态返回给 Codex。

![Codex 供应商列表里的需要路由标记](../images/codex-claude-routing/01-codex-providers-require-routing.png)

这条链路主要分成四步：

1. Codex 接管时，本地配置会被写成 `http://127.0.0.1:15721/v1`，并强制保持 `wire_api = "responses"`。
2. 供应商的上游格式 `anthropic` 会告诉路由：真实上游说的是 Anthropic Messages 协议。
3. 路由把 `/responses` 改写到 `/v1/messages`，并把 Responses 请求体转换成 Anthropic 请求体。
4. 上游返回后，路由再把 Anthropic 的 JSON 或 SSE 转回 Codex 能理解的 Responses JSON/SSE——推理内容、工具调用、图片都在转换范围内。

## 准备工作

你需要先准备好三样东西：

- 已安装并能启动的 CC Switch（3.17.0 及以上）。
- 已安装 Codex CLI，并至少运行过一次，让 `~/.codex/` 目录结构存在。
- 一个能访问 Anthropic Messages 协议端点（`/v1/messages`）的 API Key——来自某个 Claude 系中转网关，或企业内部的 Claude 网关；端点地址和认证方式以网关文档为准。注意：部分供应商会限制其 Claude API 只能在 Claude Code 中使用，这类 Key 走 Codex 可能会报错，拿不准就先咨询供应商。

Codex 页签目前没有 Anthropic 内置预设，下面走「自定义配置」路径，全程也就四五个字段。

## 第一步：添加 Codex 供应商

打开 CC Switch，切到顶部的 `Codex` 标签，点击右上角的加号添加供应商，保持默认的 `自定义配置`，然后填写：

- **供应商名称**：随意，例如 `Claude Gateway`。
- **API Key**：你的网关 Key。真实 Key 只保存在 CC Switch 里，由本地路由转发时注入，不会进入 Codex 的 live 配置。
- **API 请求地址**：填网关服务根地址即可，例如 `https://claude-gateway.example.com`。带不带 `/v1` 都能被正确处理，路由会自动把请求打到 `/v1/messages`；不要自己拼 `/v1/messages`（如果网关文档给的就是完整 messages URL，打开旁边的 `完整 URL` 开关原样粘贴也可以）。地址栏下方那句「兼容 OpenAI Response 格式」的黄色提示是为 Responses 直连场景写的通用文案，选 Anthropic 格式时按本文填写即可。
- **默认模型**：填网关认识的 Claude 模型 id，例如 `claude-sonnet-5`，以网关文档给出的模型名为准。

然后展开 `高级选项`，把 `上游格式` 从默认的 `Responses（原生）` 改成 **`Anthropic Messages（需开启路由）`**。

![Claude 网关的 Codex 供应商表单](../images/codex-claude-routing/02-claude-codex-provider-form.png)

选中 Anthropic Messages 后，下方会多出三个配套字段：

![Anthropic 上游的高级选项](../images/codex-claude-routing/03-anthropic-advanced-options.png)

- **认证字段**：决定 API Key 以哪个请求头发给上游，两者只发其一，按网关文档选择。
  - `ANTHROPIC_AUTH_TOKEN（Authorization）`：发 `Authorization: Bearer <key>`，是默认值，多数 Claude 系中转网关用这种。
  - `ANTHROPIC_API_KEY（x-api-key）`：发 `x-api-key: <key>`，部分沿用 Anthropic 原生请求头约定的网关要求这种。选错通常表现为 401 / 403。
- **模拟 Claude Code 客户端**：默认关闭。仅当网关或其上游限制「只能通过 Claude Code 使用」时才打开，开启后会伪装 User-Agent、`anthropic-beta`、`x-app` 请求头，并在系统提示首行注入 Claude Code 身份。普通网关不需要开；开启后仍被拒的处理见「常见问题」。
- **最大输出 tokens**：Anthropic 协议的 `max_tokens` 是必填项，而 Codex 请求未携带输出上限时，路由按保守的 8192 兜底，长回答或深度思考可能被截断（表现为回复不完整、`stop_reason=max_tokens`）。遇到截断就在这里按模型真实上限调高，但不要超过——超了上游会直接 400。

同区的 `模型映射` 是可选项：把 `claude-opus-4-8`、`claude-sonnet-5`、`claude-haiku-4-5-20251001` 这类模型 id（以你上游认识的名字为准）逐行加进去，CC Switch 会生成模型目录让 Codex 的 `/model` 菜单能列出它们；不填也能用，Codex 会直接请求默认模型。

保存供应商后，卡片上会出现 `需要路由` 标记——这类供应商必须在本地路由运行时才能正常工作。

## 第二步：开启本地路由并接管 Codex

进入设置里的 `路由` 页面，展开 `本地路由`，完成两个开关：

1. 打开 `路由总开关`，启动本地服务（首次开启会弹出一个说明确认框）。默认地址是 `127.0.0.1:15721`。
2. 在 `路由启用` 中打开 `Codex`。如果只想让 Codex 走路由，可以保持 Claude、Gemini 关闭。

![本地路由页面中启用 Codex 接管](../images/codex-claude-routing/04-local-route-codex-takeover.png)

接管后，CC Switch 会把 Codex 的 live 配置指向本机路由（`base_url = http://127.0.0.1:15721/v1`），`auth.json` 里只有占位符。真实 Claude Key 仍保存在 CC Switch 的供应商配置里，由本地路由在转发时按你选的认证字段注入。

## 第三步：切换供应商并重启 Codex

回到 Codex 供应商列表，点击 Claude 供应商的 `启用`。如果路由没有在运行，CC Switch 会提示「此供应商使用 Anthropic Messages 接口格式，需要路由服务才能正常使用，请先启动路由」——回到第二步打开即可。

切换后建议重启当前 Codex 终端会话：`config.toml` 和模型目录是 Codex 进程启动时读取的，运行中的进程不保证热加载。

进入 Codex 后可以逐级验证：

- 配置了模型映射的话，用 `/model` 查看 Claude 模型是否已出现在菜单里；没配映射时 Codex 直接用默认模型。
- 发一个小问题，观察设置 → 路由页面的「当前 Provider」从「等待首次请求」变成你的 Claude 供应商、「总请求数」开始增长。
- 用量看板里，这些请求的模型名会如实显示为 `claude-*`，可按供应商筛选核对 token 用量。

## 能力边界与已知限制

- **提示缓存自动生效**：转换桥会按 Anthropic 标准注入 5 分钟提示缓存标记（系统提示、工具定义与对话历史），长对话不会每轮全价重发，无需任何配置。
- **推理与工具无损**：extended thinking 内容跨桥往返保留，多轮工具调用、图片与 PDF 输入都被完整转换。
- **支持 `[1m]` 长上下文标记**：默认模型或模型映射里的模型 id 以 `[1m]` 结尾（如 `claude-sonnet-5[1m]`）时，路由会剥掉标记并自动补发对应的 1M 上下文 beta 头，前提是网关支持该能力。
- **联网搜索不可用**：Anthropic 上游模式下 Codex 的内置 `web_search` 会被主动禁用——转换层无法把它翻译给 Anthropic 端点，禁用是为了不给模型呈现一个必然失败的工具。
- **截断如实上报**：上游停在输出上限或流被掐断时，Codex 会看到「未完成」而不是被伪装的成功，方便你察觉并调高最大输出 tokens。

## 常见问题

**上游返回 401 或 403**

十有八九是认证字段与网关要求不符：在 `ANTHROPIC_AUTH_TOKEN（Authorization）` 与 `ANTHROPIC_API_KEY（x-api-key）` 之间按网关文档换一个再试（多数网关用默认的 Bearer）。另外确认 Key 本身有效、有余额。

**Codex 报 404 或找不到 `/responses`**

通常是没有开启 Codex 路由接管，或者你手动把网关的地址直接写给了 Codex——Anthropic 协议的上游没有 `/responses` 端点，这样一定 404。检查 `~/.codex/config.toml` 里当前 provider 的 `base_url` 是否指向 `http://127.0.0.1:15721/v1`。

**上游返回 404（路由已开启）**

检查 API 请求地址：应该是网关的服务根地址，而不是带 `/chat/completions` 之类其它协议路径的地址。网关路径特殊时，用 `完整 URL` 开关直接粘贴完整的 messages 端点。

**回复经常中途截断**

这是默认 8192 输出上限的表现。在供应商表单高级选项的 `最大输出 tokens` 里调高（不要超过模型/网关真实上限），保存后重试。

**`/model` 看不到 Claude 模型**

确认模型映射里已添加条目，保存供应商后重启 Codex——模型目录不会被运行中的进程热加载。默认模型不在映射里时菜单不会列出它，但直接请求仍然有效。

**联网搜索用不了**

设计如此，见「能力边界」。需要联网搜索的任务建议切回 Responses/Chat 格式的供应商。

**报错提示只能在 Claude Code 中使用**

部分供应商会限制其 Claude API 只能在 Claude Code 客户端中使用，经 Codex 走本攻略的链路时会被拒绝。可以尝试打开高级选项里的 `模拟 Claude Code 客户端` 开关；若开启后仍然报错，说明限制在供应商服务端，请咨询供应商确认你的 Key 能否在 Claude Code 之外使用。普通网关请保持该开关关闭。

## 合规提示

在「公司禁客户端、只留网关」的场景下使用前，建议确认这样做符合你所在组织的具体政策——被禁的是特定客户端还是某种使用方式，各家口径不同。使用第三方中转网关时，请阅读目标网关关于计费、合规与数据留存的条款。

## 参考链接

- [CC Switch 用户手册：代理服务](../user-manual/zh/4-proxy/4.1-service.md)
- [CC Switch 用户手册：应用路由](../user-manual/zh/4-proxy/4.2-routing.md)
- [CC Switch v3.17.0 发布说明](../release-notes/v3.17.0-zh.md)
- 功能来自社区贡献 [#5071](https://github.com/farion1231/cc-switch/pull/5071)，感谢 @yeeyzy
