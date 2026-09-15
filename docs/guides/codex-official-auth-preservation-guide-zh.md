# 使用第三方 API 时保留 Codex 远程操作和官方插件：CC Switch 配置攻略

> 适用版本：CC Switch v3.16.1 及以上。本文根据当前代码、用户手册和 v3.16.1 Release Note 整理，截图使用去敏示例数据，不包含真实 Access Token 或 API Key。

## 这篇攻略解决什么问题

很多人使用 Codex 时有两个需求：

1. 模型使用 DeepSeek、Kimi、GLM、MiniMax、硅基流动等第三方 API，或者在中转站使用 gpt 模型。
2. 保留 Codex 官方 App 的手机远程操作、官方插件等能力。

之前切换第三方供应商时，旧行为会把第三方 API Key 写进 Codex 的 `auth.json`，从而覆盖原来的官方 ChatGPT / Codex 登录缓存。这样第三方模型能用了，但依赖官方登录态的功能会消失。

v3.16.1 新增的 **Codex 应用增强**开关就是为了解决这个矛盾：让官方 Access Token 继续留在 `auth.json`，而第三方供应商信息写入 `config.toml`。这样 Codex App 仍然认为你登录的是官方账号，但实际模型请求会走 CC Switch 当前选中的第三方供应商。

v3.16.0 就有这个功能，并且默认开启，但是部分用户反映并不想要这个功能，所以在 v3.16.1 中把这个功能做成了开关。

## 先看结论

推荐顺序是：

1. 在 CC Switch 的 Codex 面板切换到 `OpenAI Official`。
2. 启动 Codex，并用官方 ChatGPT / Codex 账号登录一次，Free 订阅也可以。
3. 回到 CC Switch，打开 `设置 → 通用 → Codex 应用增强 → 切换第三方时保留官方登录`。
4. 添加或切换到第三方 Codex 供应商。
5. 如果该供应商是 Chat Completions 协议，例如 DeepSeek / Kimi / MiniMax，需要同时开启本地路由并启用 Codex 接管。
6. 重启 Codex，让 `config.toml` 和模型目录重新加载。

![设置里的 Codex 应用增强开关](../images/codex-official-auth-preservation/01-codex-app-enhancement-setting.png)

## 准备工作

你需要准备：

- CC Switch v3.16.1 或更新版本。
- 已安装并能启动的 Codex（建议 app 和 cli 都安装）。
- 一个可以登录 Codex 的官方 ChatGPT / Codex 账号，Free 订阅即可。
- 一个第三方 API Key，例如 DeepSeek、Kimi、GLM、MiniMax、OpenRouter、硅基流动等。

请不要手动复制或分享 `~/.codex/auth.json` 的内容。里面保存的是官方登录缓存和 Access Token，属于敏感信息。

## 第一步：先切回 OpenAI Official 并完成官方登录

打开 CC Switch，切到顶部的 `Codex` 标签页。先选择 `OpenAI Official` 供应商（如果没有的话，就在预设供应商当中添加一个），并把它设为当前供应商。

![Codex 供应商列表中的 OpenAI Official 与第三方供应商](../images/codex-deepseek-routing/01-codex-providers-require-routing.png)

接着启动 Codex（建议启动 cli），按 Codex 的官方登录流程登录你的 ChatGPT / Codex 账号。这个账号可以是 Free 订阅；在这个方案里，它主要负责保留 Codex 官方 App 需要识别的登录身份，不负责第三方模型的计费。

登录完成后，Codex 会在 `~/.codex/auth.json` 中保存官方登录缓存。后面的关键点就是：不要再让第三方供应商切换覆盖这个文件。

## 第二步：开启 Codex 应用增强

回到 CC Switch，进入：

```text
设置 → 通用 → Codex 应用增强
```

打开：

```text
切换第三方时保留官方登录
```

这个开关默认关闭，是因为部分用户并不想要这个功能。只有在你明确需要“第三方 API + 官方远程操作 / 官方插件”同时存在时，才需要开启它。

开启后，后端切换 Codex 第三方供应商时会走 config-only 写入路径：

- `auth.json`：继续保留官方 ChatGPT / Codex 登录缓存。
- `config.toml`：写入当前第三方供应商的模型、endpoint、`model_provider` 和 provider-scoped `experimental_bearer_token`。

## 第三步：添加第三方 Codex 供应商

回到 Codex 面板，点击右上角的加号添加供应商。推荐优先使用内置预设，例如 DeepSeek、Kimi、MiniMax、GLM、SiliconFlow 等。

以 DeepSeek 为例，选择预设后只需要填 API Key。预设会自动配置 base URL、默认模型、模型映射表和“需要本地路由映射”。

![DeepSeek Codex 供应商表单](../images/codex-deepseek-routing/02-deepseek-codex-routing-form.png)

如果你的第三方供应商原生支持 OpenAI Responses API（比如提供 gpt 模型的中转站），可以不启用本地路由。
如果它只支持 OpenAI Chat Completions，例如常见的 DeepSeek / Kimi / MiniMax 路径，就必须启用本地路由，让 CC Switch 把 Codex 的 Responses 请求转换成 Chat Completions 请求。

## 第四步：需要时开启本地路由并接管 Codex

进入：

```text
设置 → 路由 → 本地路由
```

完成两件事：

1. 打开 `路由总开关`，启动本地服务。默认地址通常是 `127.0.0.1:15721`。
2. 在 `路由启用` 中打开 `Codex`。

![本地路由页面中启用 Codex 接管](../images/codex-deepseek-routing/03-local-route-codex-takeover.png)

接管后，Codex 的 live `config.toml` 会临时指向 CC Switch 本地路由。真实第三方 API Key 仍然存储在 CC Switch 的供应商配置中，切换供应商时再投影到 `config.toml` 的 `experimental_bearer_token`。

## 第五步：切换第三方供应商并重启 Codex

回到 Codex 供应商列表，启用你刚添加的第三方供应商。切换完成后建议重启 Codex，原因有两个：

- Codex 在启动时读取 `config.toml`。
- Codex 的 `/model` 菜单通常需要重启后才会重新加载 `model_catalog_json`。

重启后，你可以做一个简单验证：

- 在 Codex App 里，账号信息仍然显示官方账号，这是预期行为。
- 在 CC Switch 里，当前 Codex 供应商显示为第三方供应商。
- 如果开启了本地路由，请求日志或路由统计会看到 Codex 请求经过本地路由。
- 第三方供应商后台或余额记录会出现实际模型请求。

## 背后的原理

Codex 的配置主要分成两个文件：

```text
~/.codex/auth.json
~/.codex/config.toml
```

这两个文件承担的职责不同：

- `auth.json` 保存官方 ChatGPT / Codex 登录缓存，也就是 Codex App 识别官方账号、远程操作和官方插件所需的登录材料。
- `config.toml` 保存当前模型供应商、base URL、模型、模型目录和 provider-scoped token 等运行配置。

开启 `切换第三方时保留官方登录` 后，CC Switch 的切换逻辑会把第三方供应商 API Key 从供应商配置中取出，写到 `config.toml` 的当前 provider 下：

```toml
model_provider = "custom"

[model_providers.custom]
name = "DeepSeek"
base_url = "https://api.deepseek.com"
wire_api = "responses"
experimental_bearer_token = "sk-..."
```

同时，`auth.json` 保持官方登录缓存不变。于是 Codex App 侧依然能识别官方账号；而模型请求会根据 `config.toml` 的当前 provider 和 base URL 走第三方 API。

如果供应商是 Chat Completions 协议，CC Switch 本地路由会再做一层转换：

```text
Codex Responses 请求
        ↓
CC Switch 本地路由
        ↓
第三方 Chat Completions API
        ↓
转换回 Codex Responses 响应
```

这就是为什么你既能继续使用官方插件 / 手机远程操作，又能把模型流量切到第三方 API。

## 需要理解的副作用

### Codex 里显示的账号始终是官方账号

这是最容易误解的一点。开启该能力后，Codex App 看到的是 `auth.json` 里的官方登录态，所以它会继续显示官方账号信息。

但这不代表模型请求还在走官方 OpenAI。实际流量以 CC Switch 当前 Codex 供应商、`config.toml` 和本地路由日志为准。

### 不要用 Codex 账号信息判断计费方

如果你切到 DeepSeek，Codex 里仍然显示官方账号，但模型请求会走 DeepSeek API。计费、限额、错误码和数据策略都应按第三方供应商理解。可以查看设置用量面板里的具体请求信息。

### 修改模型映射后要重启 Codex

Codex 的模型目录是启动时读取的。即使 CC Switch 已经生成了新的模型目录，正在运行的 Codex 也不一定会热加载，所以修改模型映射后请重启 Codex。

### 关闭开关会回到旧行为

如果关闭 `切换第三方时保留官方登录`，第三方供应商切换会沿用兼容旧版本的行为，可能重新写入 `auth.json`。如果你的目标是长期保留官方远程操作和官方插件，建议保持该开关开启。

## 常见问题

**我已经切到第三方 API，为什么 Codex 还显示官方账号？**

这是预期行为。官方账号信息来自 `auth.json`，模型请求的实际供应商来自 `config.toml` 和 CC Switch 当前供应商。

**Free 订阅真的可以吗？**

可以。这里的官方账号主要用于获取并保留 Codex App 需要的官方登录态。第三方模型请求使用的是你在 CC Switch 里配置的第三方 API Key。

**开启后官方插件或手机远程操作还是不可用怎么办？**

先切回 `OpenAI Official`，重新启动 Codex 并完成一次官方登录；然后确认 CC Switch 的 `设置 → 通用 → Codex 应用增强 → 切换第三方时保留官方登录` 已开启，再切回第三方供应商。

**第三方请求 404、模型列表不对或流式响应异常怎么办？**

如果该供应商是 Chat Completions 协议，请确认供应商表单里开启了 `需要本地路由映射`，并且 `设置 → 路由` 里已经启动路由总开关、启用 Codex 接管。

**可以在本地路由模式下切回 OpenAI Official 吗？**

不建议。CC Switch 会尽量阻止在本地路由接管模式下切到官方供应商，因为用代理访问官方 API 可能带来账号风险。建议官方登录只用于保留 `auth.json`，模型流量则切到第三方供应商。

**为什么流程做的这么复杂？可以简化吗？**

因为 Codex 增强开关和路由接管等一系列功能，如果用户并不需要的话，默认打开会带来不必要的麻烦，所以都做成了开关形式。

## 参考链接

- [Codex 桌面应用里看不到自定义模型？（常见问题）](./codex-desktop-custom-model-visibility-zh.md)
- [Codex DeepSeek 本地路由实战攻略](./codex-deepseek-routing-guide-zh.md)
- [添加 Codex 供应商：Chat Completions 路由与模型映射](../user-manual/zh/2-providers/2.1-add.md)
- [本地代理服务](../user-manual/zh/4-proxy/4.1-service.md)
- [本地路由](../user-manual/zh/4-proxy/4.2-routing.md)
- [CC Switch v3.16.1 Release Note](../release-notes/v3.16.1-zh.md)
