# 跨模型故障转移使用指南（Anthropic 兼容路线）

> 适用版本：cc-switch v3.16+
> 目标读者：希望「当首选供应商（如 OpenRouter 代理的 Claude）不可用时，
> 自动降级到另一个模型（如 DeepSeek / GLM / Qwen）」的用户。

cc-switch 的故障转移**不要求队列里的供应商都是同一个模型**——只要它们都是
同一个应用（如 Claude）下、且都讲 **Anthropic 兼容协议**的供应商，就能混在一个
故障转移队列里。代理在转发时会按每个供应商配置的 `ANTHROPIC_MODEL` 把请求重写到
它真正的上游模型。因此「OpenRouter-Claude 挂了 → 自动切到 DeepSeek」是开箱即用的，
**无需改任何代码，只需正确配置供应商**。

---

## 一、前提

你的备用模型必须提供 **Anthropic 兼容接口**（即可以填 `ANTHROPIC_BASE_URL`、
用 Anthropic Messages 协议调用）。大多数国产模型（DeepSeek、智谱 GLM、通义 Qwen 等）
都提供这种兼容端点。如果你的备用模型**只有 OpenAI 原生接口、没有 Anthropic 兼容端**，
本指南不适用（那需要代理做协议转换，属于未实现的「方案 B」）。

---

## 二、配置步骤

### 1. 建立首选供应商（P1）

在 **Claude** 分类下新建供应商 A（例如你现用的 OpenRouter）：

```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "https://openrouter.ai/api/v1",
    "ANTHROPIC_AUTH_TOKEN": "sk-or-...",
    "ANTHROPIC_MODEL": "anthropic/claude-sonnet-4.5"
  }
}
```

### 2. 建立备用供应商（P2，跨模型）

同样在 **Claude** 分类下，新建供应商 B，指向 DeepSeek 的 Anthropic 兼容端：

```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic",
    "ANTHROPIC_AUTH_TOKEN": "sk-...(DeepSeek key)",
    "ANTHROPIC_MODEL": "deepseek-chat"
  }
}
```

> ⚠️ `ANTHROPIC_BASE_URL` 与 `ANTHROPIC_MODEL` 请以 DeepSeek 官方文档为准填写。
> 关键点：**`ANTHROPIC_MODEL` 填备用模型的真实模型名**，代理会把入站的
> `claude-*` 请求重写成这个模型名再转发。

可按同样方式继续加 P3、P4（GLM、Qwen……）。

### 3. 开启代理接管 + 自动故障转移

1. 进入「代理」面板，开启 Claude 的**代理接管**。
2. 打开 **自动故障转移** 开关。
3. 在故障转移队列里把 A、B（、C……）都加进来，按你想要的优先级排序
   （顺序与首页供应商列表的拖拽顺序一致，P1 在最前）。

队列里每个条目会显示它**实际映射到的模型**（如 `→ deepseek-chat`），
方便你确认降级链路是否符合预期。

---

## 三、它是怎么工作的（原理）

```
Claude Code → 本地代理(Anthropic协议入站)
   │  自动故障转移开启时，代理按队列顺序选候选：
   ▼
P1 供应商 A (OpenRouter)  ── 连续失败触发熔断 ──┐
                                               │ 自动转移
P2 供应商 B (DeepSeek)  ◄───────────────────────┘
   │  apply_model_mapping: claude-sonnet-* → deepseek-chat
   ▼  转发到 DeepSeek 的 Anthropic 兼容端，响应原样返回给 Claude Code
```

- 每个供应商**独立熔断**（按 provider_id 隔离），互不影响。
- A 恢复后，代理会在健康检查/重置时**自动切回优先级更高的 A**。
- 切换会反映在托盘菜单和界面「当前供应商」上。

---

## 四、验证步骤（手测 / e2e）

> 用于确认你的配置真的能跨模型降级。

1. 按上文配好 A（P1）、B（P2），开启代理接管 + 自动故障转移。
2. 让 Claude Code 走本地代理，发一条消息，确认正常（此时应由 A=OpenRouter 响应）。
3. **制造 A 故障**：把 A 的 `ANTHROPIC_BASE_URL` 临时改成一个无效地址
   （或断开其网络），保存。
4. 再发消息：连续失败几次后 A 熔断，代理应**自动切到 B=DeepSeek**，
   Claude Code 继续正常对话（回答风格会变成 DeepSeek 的）。界面「当前供应商」切到 B。
5. **恢复 A**：把 base_url 改回正确值，在界面上重置 A 的熔断器。
   代理应在恢复后自动切回优先级更高的 A。

如果第 4 步成功切到 B 且能正常对话，说明跨模型故障转移已生效。

---

## 五、常见问题

- **Q：备用模型回答质量/能力和 Claude 不同？**
  A：正常。降级是「保可用」而非「保等价」，DeepSeek/GLM 等的工具调用、思考等行为
  与 Claude 不完全一致。建议把能力最接近、最稳定的放在 P2。

- **Q：可以跨到 Codex/Gemini 协议的供应商吗？**
  A：当前路线**不可以**。本功能只在「同为 Anthropic 兼容协议」的供应商间转移。
  跨协议（Anthropic↔OpenAI↔Gemini）需要代理做协议桥接，尚未实现。

- **Q：队列里看不到模型名？**
  A：模型名读自供应商 `settingsConfig.env.ANTHROPIC_MODEL`。若该供应商没填这个字段
  （例如依赖上游默认模型），队列项就不显示 `→ 模型` 徽标，属正常。
