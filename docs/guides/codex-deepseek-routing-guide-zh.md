# 在 Codex 中用 DeepSeek 这类 Chat 格式 API：CC Switch 本地路由攻略

> 适用版本：CC Switch 3.19.1 及以上。本文根据仓库内文档与代码整理。截图使用去敏示例数据生成，避免泄露真实 API Key 或账户余额。
>
> **3.19.1 起有重要变化**：DeepSeek 预设已改为原生 Responses 直连，不再需要本地路由。但这条路由转换的路径并没有作废——它仍是 `deepseek-v4-pro`、升级前已保存的供应商，以及 Kimi、智谱 GLM 等 Chat 格式供应商的必经之路。请先读下一节，确认你属于哪种情况。

## 先确认你是否还需要这篇攻略

判断方法只有一个，看 Codex 供应商卡片上有没有 `需要路由` 徽章：

![Codex 供应商列表里的需要路由标记](../images/codex-deepseek-routing/01-codex-providers-require-routing.png)

- **带 `需要路由` 徽章** → 这个供应商走 Chat 格式，本文全部适用。
- **没有徽章** → 它已经是 Responses 原生直连，本文的路由步骤对它没有意义，可以直接用。
- **带 `不支持路由` 徽章** → 这是官方供应商，CC Switch 会阻止它走本地路由（见文末常见问题）。

徽章由供应商保存时记录的 API 格式决定，所以升级 CC Switch **不会**改变已有供应商的行为。具体到 DeepSeek，升级到 3.19.1 之后有三种情况：

| 你的情况 | 是否需要路由 | 说明 |
|---|---|---|
| 3.19.1 之前保存的 DeepSeek 供应商 | **需要**，仍带徽章 | 预设改动只影响新建的供应商，已保存的配置原样保留；想改走直连见第一步末尾 |
| 3.19.1 之后用预设新建的 DeepSeek | 不需要 | 直连 `api.deepseek.com`，并会拿到 DeepSeek 官方的模型目录 |
| 想用 `deepseek-v4-pro` | **需要** | DeepSeek 官方尚未为该模型开通 Codex 集成（官方预计 2026 年 8 月初），直连会上游报错；必须走 Chat + 路由 |

除 DeepSeek 外，Kimi、智谱 GLM、SiliconFlow、ModelScope 等大量供应商仍是 Chat 格式，本文对它们完全适用——把下文中的 DeepSeek 换成对应预设即可。

## 为什么需要本地路由

新版 Codex CLI 面向的是 OpenAI Responses API，而很多供应商实际暴露的是 OpenAI Chat Completions 形态，也就是 `/chat/completions`。这两种协议的请求体、流式事件和返回结构不同，直接把 Chat 接口填进 Codex 配置里，常见结果就是模型列表不对、请求 404/400，或者流式响应无法被 Codex 正确解析。

CC Switch 的做法是让 Codex 始终连本机路由，仍以 Responses API 发送请求；路由在内部识别当前供应商是否是 Chat 格式，再把请求改写成 Chat Completions 发给上游，最后把 Chat 响应转换回 Responses 形态返回给 Codex。

这条链路主要分成四步：

1. Codex 接管时，本地配置会被写成 `http://127.0.0.1:15721/v1`，并强制保持 `wire_api = "responses"`。
2. Provider 的 `meta.apiFormat = "openai_chat"` 会告诉路由：真实上游是 Chat Completions。
3. 路由把 `/responses` 或 `/v1/responses` 改写到 `/chat/completions`，并把 Responses 请求体转换成 Chat 请求体。
4. 上游返回后，路由再把 Chat 的 JSON 或 SSE 转回 Codex 能理解的 Responses JSON/SSE。

供应商原生就是 Responses 的（如现在的 DeepSeek 预设），第 2～4 步不发生：请求直接发往上游，不做任何格式改写。

## 准备工作

你需要先准备好三样东西：

- 已安装并能启动的 CC Switch。
- 已安装 Codex CLI，并至少运行过一次，让 `~/.codex/config.toml` 目录结构存在。
- 目标供应商的 API Key。

以 DeepSeek 为例，官方文档写明 OpenAI 兼容 base URL 是 `https://api.deepseek.com`（其他供应商常见的是带 `/v1` 或更长路径的 base URL，例如智谱 GLM 是 `https://open.bigmodel.cn/api/coding/paas/v4`），Chat API 路径是 `/chat/completions`。CC Switch 的预设已按这些信息配好，请优先使用预设，不需要手动拼接口路径。

## 第一步：添加 Codex 供应商

打开 CC Switch，切到顶部的 `Codex` 标签，点击右上角的加号添加供应商。

**用预设**（推荐）：在预设列表里选中目标供应商，填入 API Key，保存即可。预设已经内置请求地址、默认模型、模型菜单，并会自动设好上游格式；Chat 格式的预设保存后卡片上就会出现 `需要路由` 徽章。思考参数（thinking / reasoning）预设已自动配置好，不需要手动填。

**用自定义配置**：按对方文档填 API Key 和 base URL，然后展开表单底部的 `高级选项`，把 `上游格式` 选为 `Chat Completions（需开启路由）`。这个下拉共有三个选项：

- `Responses（原生）`——上游原生支持 Responses API，直连不转换，无需路由。
- `Chat Completions（需开启路由）`——本文讲的情况。
- `Anthropic Messages（需开启路由）`——上游只提供原生 Anthropic 协议，同样由路由转换。

只有 `Responses（原生）` 不需要开启路由接管，另外两个都需要。自定义供应商的思考参数由 CC Switch 按名称与地址自动推断，只有在识别不准时才需要展开 `思考能力` 手动覆盖。

> **改造已有的 DeepSeek 供应商**：把 `上游格式` 改成 `Responses（原生）` 即可，不必删掉重建。下次切换到它时，CC Switch 会认出 `deepseek.com` 地址并套用 DeepSeek 官方的模型目录，freeform `apply_patch`、GPT-5 harness、low/high/max 思考档与 web_search 都会照常生效。
>
> 唯一的小差别是上下文窗口：供应商自己保存的模型行优先级更高，3.19.1 之前存下的 `1000000` 会盖掉官方声明的 `1048576`，少 4 万多 token。介意的话，在 `高级选项` → `模型映射` 里把该行的 `上下文窗口` 改成 `1048576` 就行，或者干脆用预设新建一个。
>
> 反过来，想用 `deepseek-v4-pro`，就把 `上游格式` 改回 `Chat Completions`。
>
> 另外，直连所用的官方模型目录要求 Codex CLI **0.144.0 或更新**（它带的 freeform `apply_patch` 注册需要这个版本），CC Switch 不会替你校验；生成的目录文件也会涨到 75 KB 左右，因为其中包含完整的 GPT-5 harness 文本。

## 第二步：开启本地路由并接管 Codex

进入设置里的 `路由` 页面，展开 `本地路由`，完成两个开关：

1. 打开 `路由总开关`，启动本地服务。默认地址是 `127.0.0.1:15721`。
2. 在 `路由启用` 中打开 `Codex`。如果只想让 Codex 走路由，可以保持 Claude、Gemini 关闭。

![本地路由页面中启用 Codex 接管](../images/codex-deepseek-routing/03-local-route-codex-takeover.png)

接管后，CC Switch 会把 Codex 的 live 配置指向本机路由，并用占位符管理认证。真实 API Key 仍保存在 CC Switch 的 Provider 配置里，由本地路由在转发时注入，不需要你把 Key 暴露给 Codex live 配置。

## 第三步：切换供应商并重启 Codex

回到 Codex 供应商列表，点击目标供应商的 `启用`。如果它带 `需要路由` 标记而路由没有启动，CC Switch 会弹出“需要路由服务才能正常使用”的提示。

切换后建议重启当前 Codex 终端会话。原因是：

- Codex 进程可能已经读取过旧的 `config.toml`。
- `model_catalog_json` 生成后，`/model` 菜单通常需要新进程才能刷新。

进入 Codex 后，可以用 `/model` 查看当前模型是否来自对应预设。随后发一个小问题，确认路由面板的请求数增长，或者在用量/请求日志里看到 Codex 请求即可。

## 走直连之后，用量归属会变

这一点值得单独提醒：供应商改走直连后，它的请求不再经过本地路由，按请求计费的代理用量统计也就看不到它了。

用量本身不会丢——Codex 的会话日志导入照常记录——但这条路径不携带供应商身份：所有没走本地代理的 Codex 用量会一起归入名为 `Codex (Session)` 的条目。**要区分它们，看模型**：每条用量记录都带着自己的模型 ID，用量面板的「模型统计」按模型逐行列出，费用与 token 都是分开的。

如果你确实需要按供应商维度对账（比如比较多个聚合商上的同一个模型），那就保持 `上游格式` 为 Chat 并开着路由接管。

## 常见问题

**Codex 报 404 或找不到 `/responses`**

通常是没有开启 Codex 接管，或者你手动把上游 Chat base URL 直接写给了 Codex。检查 `~/.codex/config.toml` 是否指向 `http://127.0.0.1:15721/v1`。

**上游报 404**

如果用的是内置预设，先确认当前供应商确实来自预设，并且 Codex 路由已启用。只有在使用自定义供应商时，才需要额外检查 base URL：它应该是对方文档给出的服务端点，而不是带 `/chat/completions` 的完整接口路径。

**切到 `deepseek-v4-pro` 后上游报错**

DeepSeek 官方尚未为该模型开通 Codex 集成。把这个供应商的 `上游格式` 改回 `Chat Completions（需开启路由）` 并开启路由接管即可——这正是 3.19.1 之前 DeepSeek 走的路径，路由的 Responses→Chat 转换照常支持 pro。或者改用 `deepseek-v4-flash`，它是预设默认值，不受影响。

**`/model` 看不到供应商的模型**

保存供应商后重启 Codex。CC Switch 会生成 `cc-switch-model-catalog.json` 并把路径写入 `model_catalog_json`，但正在运行的 Codex 进程不一定会热加载模型目录。
目前 Codex app 不支持多模型选择，默认使用配置的第一个模型。

**开了路由但请求仍走错供应商**

确认三处状态一致：Codex 标签下当前供应商正确；本地路由服务正在运行；`路由启用` 里 Codex 开关已打开。

**可以用官方 OpenAI Codex 账号走本地路由吗**

不建议。CC Switch 会在本地路由接管模式下阻止切到官方供应商，因为用代理访问官方 API 可能带来账号风险。路由主要用于第三方、聚合或协议转换场景。

## 参考链接

- [CC Switch 用户手册：添加供应商](../user-manual/zh/2-providers/2.1-add.md)
- [CC Switch 用户手册：代理服务](../user-manual/zh/4-proxy/4.1-service.md)
- [CC Switch 用户手册：应用路由](../user-manual/zh/4-proxy/4.2-routing.md)
- [DeepSeek API 文档：Integrate with Codex](https://api-docs.deepseek.com/quick_start/agent_integrations/codex/)（官方 Codex 集成说明，含 `wire_api = "responses"` 与模型支持范围）
- [DeepSeek API 文档：Using the Responses API](https://api-docs.deepseek.com/guides/responses_api/)
- [DeepSeek API 文档：Your First API Call](https://api-docs.deepseek.com/)
- [DeepSeek API 文档：Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)
- [DeepSeek API 文档：Multi-round Conversation](https://api-docs.deepseek.com/guides/multi_round_chat)
