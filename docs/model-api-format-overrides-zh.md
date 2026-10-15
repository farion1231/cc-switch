# 模型选择协议，原有高级参数按协议分组

同一供应商可能为不同模型提供不同协议。新增的“模型协议覆盖”只保存模型 ID 到上游协议的映射；原有与协议有关的高级参数按协议分组，同组模型共用，不为每个模型复制一套参数。

Codex → 本地代理（Responses）→ 完成模型映射或 Stack 路由 → 按最终模型选择上游协议 → 复用现有转换器。响应转换回 Codex 的 Responses；Claude Code 下游仍使用 Messages。

## 参数放在哪里

| 设置位置                       | 作用范围                                                                                  |
| ------------------------------ | ----------------------------------------------------------------------------------------- |
| 模型协议覆盖                   | 模型选择 Anthropic Messages、OpenAI Chat Completions 或 OpenAI Responses                  |
| Messages 上游参数组            | 使用 Messages 的模型共用现有认证方式、模拟 Claude Code、输出上限；后两项仅 Codex 转换使用 |
| Chat 上游参数组                | 使用 Chat 的模型共用原有思考模式、思考等级支持开关、缓存路由等设置，复用既有参数映射规则  |
| Responses 上游组               | 沿用原生处理规则，无额外转换参数                                                          |
| 所有协议共用的设置             | 原有 User-Agent、通用请求头/请求体覆盖等设置                                              |
| 原版模型目录每一行的“思考等级” | **该模型自己的可选深度和默认深度**，不合并为供应商或协议的统一深度                        |

协议组控制“怎么把请求表达给上游”，模型目录控制“这个模型可选哪些深度、默认哪个深度”。Codex 对模型选定的 `reasoning.effort` 继续按现有协议转换规则发送；`zen` 模式继续按模型目录声明的合法档位校准。没有新增思考参数映射机制，也没有新增逐模型高级参数结构。

没有模型协议覆盖时，保留原版高级设置布局。开启覆盖后，在普通和 Stack 编辑布局中按实际使用的协议显示原有设置，一组只出现一次。切换供应商默认协议不会丢弃覆盖模型仍需要的另一协议设置。

## 配置示例

以下仅展示相关字段。Base URL、API Key 仍在原有位置配置，所有模型共用。模型支持的真实档位应按供应商能力填写。

```json
{
  "settingsConfig": {
    "modelCatalog": {
      "models": [
        {
          "model": "messages-model",
          "reasoningLevels": ["low", "medium", "high"],
          "defaultReasoningLevel": "medium"
        },
        {
          "model": "chat-model",
          "reasoningLevels": ["low", "high"],
          "defaultReasoningLevel": "high"
        }
      ]
    }
  },
  "meta": {
    "apiFormat": "openai_chat",
    "modelApiFormats": {
      "messages-model": "anthropic",
      "chat-model": "openai_chat"
    },
    "apiKeyField": "ANTHROPIC_API_KEY",
    "maxOutputTokens": 4096,
    "codexChatReasoning": {
      "supportsThinking": true,
      "supportsEffort": true,
      "thinkingParam": "none",
      "effortParam": "reasoning_effort",
      "effortValueMode": "passthrough"
    }
  }
}
```

只有 `meta.modelApiFormats` 是新增配置；其余均复用项目现有字段。模型目录不因添加协议覆盖自动增加模型。模型 ID 在映射、Stack 路由完成后精确匹配，区分大小写，忽略末尾 `[1M]` 标记。删除模型覆盖后恢复供应商默认协议，原有协议参数保留其原语义。

## 首版边界

- 仅开放普通第三方 Claude Code、Codex 供应商。官方账号、托管 OAuth、Copilot、云供应商沿用原协议规则。
- 需要本地路由；完整端点 URL 与模型协议覆盖互斥。空白新增行不保存；缺少模型、缺少协议或重复模型会提示修正。
- Claude Code 沿用自己的转换参数规则，不能把 Codex Chat 的参数设置当成 Claude Code 的参数设置。
- 同一协议的模型共用原有高级参数，不提供逐模型参数名或输出上限覆盖。每个模型的可选和默认思考深度仍可独立设置。
- 通用请求头/请求体覆盖保持供应商级作用域，最后应用于所有模型，不会随模型协议自动分类；避免在其中手动改模型或混入仅适用于某协议的字段。
- Codex 模型目录继续按每个模型有效协议生成工具能力；发布 Messages 模型时，沿用禁用全局 hosted web search 的兼容策略。
- 重试、故障转移按实际目标供应商重新解析；旧供应商未配置覆盖时保持原行为。无需数据库迁移，不探测协议、不新增转换器。

## 验证

模拟上游验证两个客户端 × 三种协议 × 流式/非流式的实际转发与响应转换，包含认证头、工具历史/调用、模型映射、`[1M]`、默认回退、故障转移和目录能力。附加测试验证同协议模型共用参数、不同模型请求各自的思考深度、两种编辑布局中的协议分组，以及模型目录各行独立的可选/默认深度。

真实网关短编码请求已验证 OpenCode Go 的 Haiku Messages、DS Chat 和 Codex reasoning 转换；未启动完整 CLI 会话或验证真实工具执行，具体代码版本与结果见 PR。
