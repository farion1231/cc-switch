# 在 Qoder CN 桌面版切换模型供应商

Qoder CN 桌面版可以用一个 OpenAI Responses 自定义模型连接 CC Switch。完成一次配置后，可以在 CC Switch 切换上游供应商，Qoder CN 继续使用同一个模型入口和会话。

## 配置 CC Switch

1. 从侧栏打开「Qoder CN Desktop」。通过页头的添加按钮填写上游协议、API 地址、模型 ID 和 API Key。也可以在添加表单里复制已有的 Claude Code API Key 供应商。
2. 用供应商卡片的「切换」按钮选中要使用的供应商。展开列表上方的「本地网关」，点「启动网关」。这里提供 Qoder CN 需要的 Base URL、Model ID 和本地网关 API Key。

上游支持 OpenAI Responses、OpenAI Chat Completions 和 Anthropic Messages。每张卡片的模型 ID 应填写对应供应商实际开放的模型。

## 配置 Qoder CN

1. 打开「设置 → 模型 → 添加模型」。供应商选择 **OpenAI Compatible**，API 类型选择 **Responses API**。
2. 从 CC Switch 页面复制 Base URL、Model ID 和 API Key。校验并添加模型后，在 Qoder CN 的聊天模型菜单选择 `cc-switch-qoder`。
3. 要换供应商，在 CC Switch 的「Qoder CN Desktop」供应商列表点击另一张卡片的「切换」。回到原会话继续发送消息即可。

CC Switch 需要保持运行。页面上的 API Key 只用于 Qoder CN 连接本地网关，服务商的 API Key 由 CC Switch 在转发时使用。点「停用网关」可撤销 Qoder CN 对这个入口的访问。

可参考 [Qoder CN 自定义模型文档](https://docs.qoder.cn/qoder/custom-models)中的校验步骤。
