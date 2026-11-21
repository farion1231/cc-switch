# 在 Qoder CN 桌面版切换模型供应商

Qoder CN 桌面版可以用一个 OpenAI Responses 自定义模型连接 CC Switch。完成一次配置后，可以在 CC Switch 切换上游供应商，Qoder CN 继续使用同一个模型入口和会话。

## 配置 CC Switch

1. 从侧栏打开「Qoder CN 桌面版」。点「添加供应商」，填写上游的协议、Base URL、模型 ID 和 API Key。也可以在同一页面复制已有的 Claude Code API Key 供应商。
2. 在要使用的供应商上点「切换到此」，再点「启动网关」。页面会显示 Base URL、Model ID 和本地网关的 API Key。

上游支持 OpenAI Responses、OpenAI Chat Completions 和 Anthropic Messages。每张卡片的模型 ID 应填写对应供应商实际开放的模型。

## 配置 Qoder CN

1. 打开「设置 → 模型 → 添加模型」。供应商选择 **OpenAI Compatible**，API 类型选择 **Responses API**。
2. 从 CC Switch 页面复制 Base URL、Model ID 和 API Key。校验并添加模型后，在 Qoder CN 的聊天模型菜单选择 `cc-switch-qoder`。
3. 要换供应商，在 CC Switch 的「Qoder CN 桌面版」页面点击另一张卡片的「切换到此」。继续发送消息即可验证切换结果。

CC Switch 需要保持运行。页面上的 API Key 只用于 Qoder CN 连接本地网关，服务商的 API Key 由 CC Switch 在转发时使用。点「停用网关」可撤销 Qoder CN 对这个入口的访问。

可参考 [Qoder CN 自定义模型文档](https://docs.qoder.cn/qoder/custom-models)中的校验步骤。
