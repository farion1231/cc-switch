# Codex 桌面应用里看不到自定义模型？（常见问题）

> 适用版本：CC Switch v3.16.1 及以上。本文解释「为什么 Codex 桌面应用看不到自定义模型」以及可用的缓解办法；详细的图文配置步骤见 [使用第三方 API 时保留 Codex 远程操作和官方插件](./codex-official-auth-preservation-guide-zh.md)。

## 现象

在 CC Switch 里把 Codex 切换到第三方 / 自定义模型（DeepSeek、Kimi、GLM、MiniMax、中转站等）后：

- **Codex 桌面应用**的模型选择器里看不到这些自定义模型，往往只剩官方默认模型，思考等级也回落到官方默认；
- 但**命令行 `codex`** 的 `/model` 里一切正常。

很多用户都遇到过这个现象，下面解释原因与办法。

## 为什么会这样

这**不是 CC Switch 的本地配置问题，也不是 CC Switch 的 bug**，而是 **Codex 桌面应用（上游闭源客户端）自身的模型门控行为**。

Codex 桌面应用的模型选择器会按你**当前的登录身份**来决定放行哪些模型：当它检测不到官方 ChatGPT / Codex 登录态时，会把选择器强制回落到官方默认模型，把你通过 `config.toml` 配置的自定义模型藏起来（思考等级也会一并回落到官方默认）。官方已把「在桌面 GUI 里暴露自定义供应商模型」标记为 not planned，因此 CC Switch 无法从桌面 GUI 层面彻底修复它。

命令行 `codex` 的 `/model` 与请求路由都能正常识别 `config.toml` 里的自定义供应商，**唯独桌面 GUI 的选择器受这层门控限制**。

## 怎么缓解：保留官方登录

办法是**保留官方登录态**，让桌面应用的门控放行你的自定义模型。要点如下（完整图文步骤见下方链接的攻略）：

1. 先在 Codex 里登录一次官方 ChatGPT / Codex（Free 订阅即可），保留官方登录态。
2. 在 CC Switch 开启 `设置 → 通用 → Codex 应用增强 → 切换第三方时保留官方登录`（**默认关闭**）。
3. 为该第三方供应商开启本地路由并接管 Codex（Chat Completions 协议的供应商如 DeepSeek / Kimi / MiniMax 必须开启）。
4. 完全退出并重启 Codex。

开启后，CC Switch 在切换第三方供应商时会保留 `~/.codex/auth.json` 里的官方登录态、把第三方 Key 写进 `config.toml`，于是桌面应用仍识别官方登录身份、门控放行，你配置的自定义模型就会重新出现在选择器里。**保留的官方 Token 不会被发往第三方**——第三方模型请求仍用你配置的 Key 经本地路由转发。

> 📖 详细图文步骤：[使用第三方 API 时保留 Codex 远程操作和官方插件](./codex-official-auth-preservation-guide-zh.md)

## 仍然看不到怎么办

- **确认开关已开**：该开关默认关闭，很多人第一次切到第三方就把官方登录态覆盖掉了，所以才看不到——按上面开启即可。
- **官方登录态会过期**：如果连续几天没用过官方登录，Token 失效后选择器可能又变空——重新登录一次官方即可恢复。
- **命令行兜底诊断**：用 `codex debug models` 可以列出 CLI 端实际可用的模型，确认模型本身已正确配置（CLI 不受此门控影响）。
- 个别 Codex 桌面版本的行为可能略有差异；这属于上游客户端范畴，CC Switch 各版本都无法从桌面 GUI 层根治。

## 参考链接

- [使用第三方 API 时保留 Codex 远程操作和官方插件](./codex-official-auth-preservation-guide-zh.md)
- [Codex DeepSeek 本地路由实战攻略](./codex-deepseek-routing-guide-zh.md)
- [本地路由](../user-manual/zh/4-proxy/4.2-routing.md)
