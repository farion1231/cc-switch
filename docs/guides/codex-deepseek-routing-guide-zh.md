# Codex 本地模型路由指南

> 适用于 CC Switch 3.16.0+。

这份文档替代旧的 DeepSeek 专用说明。Codex 仍然只连接一个 CC Switch Rust 本地代理端点，CC Switch 在代理内部根据 `body.model` 把请求分发到不同上游 route。

## 为什么需要它

Codex 客户端发送 OpenAI Responses API 请求，但很多上游只提供 Chat Completions 或 Messages 风格接口。把这些上游地址直接写进 `~/.codex/config.toml` 时，常见问题包括 `/responses` 404/400、模型列表不匹配、流式响应解析失败。

启用本地模型路由后，Codex 固定连接 CC Switch 本地代理，例如 `http://127.0.0.1:15721/v1/responses`，真实上游由 CC Switch 按模型选择。

## 运行链路

1. Codex 请求 CC Switch 本地代理。
2. CC Switch 读取请求体中的 `model`。
3. route resolver 按 `settings_config.codexRouting.routes[]` 的精确模型或前缀匹配。
4. CC Switch 生成 effective provider，写入 route 的 base URL、API format、auth、model mapping 和 capability。
5. 复用现有 forwarder 执行协议转换：
   - `openai_responses`：Responses 透传。
   - `openai_chat`：Responses 转 Chat Completions，再转回 Responses。
   - `openai_messages`：在 route 支持时转成 Messages 格式。

## 配置 route

在 Codex provider 表单中打开 **Local model routing**，每条 route 可以配置：

- 匹配规则：`match.models`、`match.prefixes`。
- 上游：`upstream.baseUrl`、`upstream.apiFormat`。
- 鉴权来源：
  - `provider_config`：使用 route 或当前 provider 的 API key。
  - `managed_codex_oauth`：使用 CC Switch 托管的 Codex OAuth。
  - `managed_account`：托管账号鉴权绑定，当前映射为 Codex OAuth。
- 模型映射：`upstream.modelMap`，例如 `codex-model=upstream-model`。
- 能力声明：text-only、image、reasoning。

第一版暂不支持 `reuse_provider:<id>`。

## 配置结构

```json
{
  "settings_config": {
    "codexRouting": {
      "enabled": true,
      "defaultRouteId": "openai",
      "routes": [
        {
          "id": "deepseek",
          "label": "DeepSeek",
          "enabled": true,
          "match": {
            "models": ["deepseek-v4-flash"],
            "prefixes": ["deepseek-"]
          },
          "upstream": {
            "baseUrl": "https://api.deepseek.com",
            "apiFormat": "openai_chat",
            "auth": { "source": "provider_config" },
            "modelMap": { "deepseek-v4-flash": "deepseek-v4-flash" }
          },
          "capabilities": {
            "textOnly": true,
            "inputModalities": ["text"],
            "supportsReasoning": true
          }
        }
      ]
    }
  }
}
```

`settings_config.codexRouting` 是新主配置。`settings_config.codexModelRoutes` 和 `settings_config.modelRoutes` 只作为旧配置只读兜底；UI 读取旧字段后，保存时会写回新 schema。

## 注意事项

- text-only route 会让 catalog 模型生成 `input_modalities=["text"]`。
- Responses -> Chat 转换会读取 route capability，避免把 `image_url` 发给 text-only 上游。
- 聊天窗口切换模型时不需要先在 GUI 切 provider，因为路由依据是请求里的 `body.model`。
