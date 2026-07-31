# 在 Codex 中使用 DeepSeek V4 Pro：CC Switch 本地路由指南

> **重要：**内置的 `DeepSeek` 主预设现在只包含 DeepSeek V4 Flash，并通过原生 Responses API 直连，**不需要本地路由**。本指南只适用于独立的 `DeepSeek V4 Pro` 预设；该模型目前通过 Chat Completions 接入，需要 CC Switch 在本地完成协议转换。

> 已保存的 DeepSeek 供应商不会随内置预设自动迁移。要使用 Flash 原生 Responses 或新的 Pro Chat 配置，请重新选择对应预设，或新建供应商。

## 先选择正确的预设

| 预设 | 模型 | 上游协议 | 是否需要本地路由 |
|------|------|----------|------------------|
| `DeepSeek` | `deepseek-v4-flash` | 原生 Responses | 否 |
| `DeepSeek V4 Pro` | `deepseek-v4-pro` | Chat Completions | 是 |

如果要使用 Flash，选择 `DeepSeek`、填写 API Key 并保存即可。其 Codex 模型目录已经声明函数调用、freeform `apply_patch`、文本 Web Search、并行工具调用，以及 `low` / `high` / `max` 思考等级。

以下步骤仅针对 `DeepSeek V4 Pro`。

## 为什么 V4 Pro 需要本地路由

Codex CLI 使用 OpenAI Responses API，而 V4 Pro 预设目前走 Chat Completions。CC Switch 会让 Codex 继续发送 Responses 请求，再在本地完成双向转换：

1. Codex 接管后，live 配置指向 `http://127.0.0.1:15721/v1`，并保留 `wire_api = "responses"`。
2. `DeepSeek V4 Pro` 预设标记为 Chat Completions 格式。
3. 本地路由把 Responses 请求转换成 Chat Completions 请求，并发送到 DeepSeek。
4. DeepSeek 返回后，路由将 JSON 或 SSE 转回 Codex 能识别的 Responses 格式。

## 准备工作

你需要：

- 已安装并能启动的 CC Switch。
- 已安装且至少运行过一次的 Codex CLI。
- DeepSeek API Key。

预设已包含 `https://api.deepseek.com` 和正确的模型名，不要手动把 `/chat/completions` 拼进 base URL。

## 第一步：添加 V4 Pro 供应商

打开 CC Switch，切到顶部的 `Codex` 标签，点击右上角的加号：

1. 选择 `DeepSeek V4 Pro` 预设。
2. 填写 DeepSeek API Key。
3. 保存供应商。

该预设会自动配置 Chat Completions 格式、`deepseek-v4-pro` 模型目录和思考参数。通常不需要手动修改高级配置。

## 第二步：开启本地路由并接管 Codex

进入设置里的 `路由` 页面，展开 `本地路由`：

1. 打开路由总开关，启动本地服务。默认地址是 `127.0.0.1:15721`。
2. 在应用路由中启用 `Codex`。

接管后，Codex 的 live 配置会指向本地路由。真实 API Key 仍保存在 CC Switch 的供应商配置里，由路由在转发时注入。

## 第三步：启用供应商并重启 Codex

回到 Codex 供应商列表，启用 `DeepSeek V4 Pro`。该预设会显示需要路由；使用期间应保持本地路由运行。

切换后重启 Codex 终端会话。Codex 进程可能已经读取旧的 `config.toml`，而 `/model` 菜单通常也要在新进程中重新加载 `model_catalog_json`。

进入 Codex 后，用 `/model` 确认当前模型为 `DeepSeek V4 Pro`，再发送一个小请求，并在 CC Switch 的路由或请求日志中确认请求到达。

## 从旧 DeepSeek 配置迁移

旧版本把 Flash 和 Pro 放在同一个 Chat 预设中。升级后，已有供应商仍保留原来的保存值，不会自动切换协议：

- 使用 Flash：重新选择 `DeepSeek` 预设或新建供应商；它通过原生 Responses 直连，不需要本地路由。
- 使用 Pro：重新选择 `DeepSeek V4 Pro` 预设或新建供应商，并保持 Codex 本地路由开启。

切换预设后请重启 Codex，以刷新 live 配置和模型目录。

## 常见问题

**我只使用 V4 Flash，也要开启 Codex 本地路由吗？**

不需要。选择主 `DeepSeek` 预设即可。Flash 原生支持 Responses，CC Switch 不会做 Chat 协议转换。

**V4 Pro 报 404 或找不到 `/responses`**

确认当前选中的是 `DeepSeek V4 Pro`，本地路由服务正在运行，并且 Codex 应用路由已启用。不要把 DeepSeek 的 Chat base URL 直接写给 Codex。

**`/model` 看不到 DeepSeek 模型**

保存并启用供应商后重启 Codex。运行中的 Codex 进程不一定会热加载模型目录。

**路由开启后请求仍走错供应商**

确认 Codex 标签下启用的是 `DeepSeek V4 Pro`，路由总开关已打开，并且应用路由中的 Codex 开关已启用。

## 参考链接

- [CC Switch 用户手册：添加供应商](../user-manual/zh/2-providers/2.1-add.md)
- [CC Switch 用户手册：代理服务](../user-manual/zh/4-proxy/4.1-service.md)
- [CC Switch 用户手册：应用路由](../user-manual/zh/4-proxy/4.2-routing.md)
- [DeepSeek：使用 Responses API](https://api-docs.deepseek.com/guides/responses_api)
- [DeepSeek：集成 Codex](https://api-docs.deepseek.com/quick_start/agent_integrations/codex)
