# Qwen Code 接入

Qwen Code 是独立应用，应用 ID 为 `qwen`（别名 `qwen-code`、`qwencode`）。接入面向 [QwenLM/qwen-code](https://github.com/QwenLM/qwen-code) 0.26+，配置目录为 `~/.qwen`，设置环境变量 `QWEN_HOME` 后全部跟随该目录（CC Switch 进程需要继承这个环境变量）。

## 使用方式

1. 在「应用」页安装 Qwen Code。CC Switch 使用官方独立安装脚本（自带运行时，脚本内部在没有独立包时回退到 npm），安装到 `~/.local/bin`；已有客户端升级使用 `qwen update`，失败时重跑官方安装脚本。手动安装可使用 `npm i -g @qwen-code/qwen-code@latest`（需要 Node 22+）或 `brew install qwen-code`。
2. 在侧栏选择 **Qwen Code**。`settings.json` 中已有的 `modelProviders` 会按 `envKey` 分组自动出现在列表中。
3. 「添加供应商」从「模型厂商」「第三方平台」「云服务商」「本地」分组中选择预设，或使用「自定义配置」。预设复用 Pi 的供应商目录，并转换为 Qwen 原生的认证类型和模型字段。
4. 在「添加供应商」中保存后，供应商及其模型写入 `modelProviders`。「可添加」分组里的卡片点击「添加」只写入原生配置、不改当前模型，模型随即出现在 Qwen Code 的 `/model` 列表中；已在原生配置中的同名条目归 Qwen Code 所有，再次添加不会用 CC Switch 的存档覆盖。多个供应商可以同时启用。
5. 已添加卡片的「更多」菜单中的「设为默认」会把 `security.auth.selectedType`、`model.name`、`model.baseUrl` 指向该供应商：当前模型属于该供应商时保留，否则选择它的第一个模型；当前模型所属的供应商以「默认」标识。卡片上的「移除」会从原生配置移除该供应商的模型和 CC Switch 写入的 Key，并保留 CC Switch 中的档案。默认供应商不能移除，需要先把其他供应商设为默认。

Qwen OAuth 免费额度已于 2026-04-15 停止，不提供官方登录入口；阿里云百炼、编程套餐等使用 API Key 的预设。

## 供应商档案格式

```jsonc
{
  "authType": "openai",            // openai | openai-responses | anthropic | gemini | vertex-ai
  "baseUrl": "https://dashscope.aliyuncs.com/compatible-mode/v1",
  "apiKey": "sk-...",              // 写入 settings.json 的 env[envKey]
  "envKey": "OPENAI_API_KEY",      // 可选：只有从原生配置导入、使用自有环境变量的供应商才有
  "models": [
    {
      "id": "qwen3-coder-plus",
      "name": "Qwen3 Coder Plus",
      "generationConfig": { "contextWindowSize": 1000000 }
      // 其余原生字段（description、capabilities、wireApi、samplingParams…）原样保留
    }
  ]
}
```

- 供应商标识（`provider key`）使用 `^[a-z0-9]+(-[a-z0-9]+)*$`。CC Switch 创建的供应商使用托管环境变量 `CC_SWITCH_QWEN_<KEY>`（大写，`-` 换成 `_`），两者一一对应。
- 原生条目通过 `envKey` 归属到供应商。`envKey` 以 `CC_SWITCH_QWEN_` 开头的分组还原为对应的供应商标识；其他分组（例如 `OPENAI_API_KEY`）以环境变量名的小写短横线形式作为标识，并在档案中保留 `envKey`。
- 模型的 `baseUrl` 与供应商相同时只存在供应商上；不同时保留在模型上。模型位于其他认证类型数组时，档案中的模型带 `authType`，写回时放回原数组。

## 配置与能力

| 能力 | 原生位置／行为 |
| --- | --- |
| 供应商和模型 | `~/.qwen/settings.json` 的 `modelProviders.<authType>[]`，Key 写入同文件的 `env` |
| 当前模型 | `security.auth.selectedType` + `model.name`（同名模型用 `model.baseUrl` 区分） |
| 协议 | `openai`（Chat Completions）、`openai-responses`、`anthropic`、`gemini`、`vertex-ai` |
| 供应商目录 | 复用 Pi 的供应商信息；AWS Bedrock 的专有签名协议不兼容，不提供该预设 |
| MCP | `settings.json` 的 `mcpServers`：stdio 用 `command`，HTTP 用 `httpUrl`，SSE 用 `url` |
| Skills | `~/.qwen/skills/`，通过统一 Skills 页面管理 |
| 全局提示词 | `~/.qwen/QWEN.md`，通过统一提示词页面管理 |
| 会话 | `<运行目录>/projects/<规范化工作目录>/chats/<sessionId>.jsonl`；运行目录依次取 `QWEN_RUNTIME_DIR`、`advanced.runtimeOutputDir`、配置目录 |
| 恢复会话 | 在原工作目录执行 `qwen --resume <sessionId>` |
| 用量 | 从 assistant 记录的 `usageMetadata` 导入输入／输出／缓存／思考 token，模型取记录上的 `model` |

`settings.json` 是 JSONC。CC Switch 只修改 `modelProviders`、`env`、`security.auth.selectedType`、`model.name`、`model.baseUrl`、`mcpServers` 中自己负责的节点，保留注释、其他键和格式；新建文件时写入 `"$version": 4`。

Qwen Code 使用原生多供应商配置，因此此接入采用共存模式。当前不提供 CC Switch 的本地路由或聚合模式。

## 数据保留规则

- `settings.env` 的优先级低于 shell 中导出的同名变量，所以托管供应商使用独立的 `CC_SWITCH_QWEN_*` 变量，不和 `OPENAI_API_KEY` 等通用变量冲突。导入的自有变量供应商只有在填写了 Key 时才写 `env`。
- 移除供应商时只删除托管变量；自有变量（例如 `OPENAI_API_KEY`）保持不动。
- 无法解析的 `settings.json` 会报错并保留原文件。供应商和 MCP 的原生写入与数据库保存之间发生失败时，恢复写入前的文件。
- 会话删除在 Qwen Code 中完成（`qwen sessions`），以保持索引与归档一致。
- 用量使用日志中当时的模型，不从当前供应商推断历史归属；缺少价格时费用为 `0`，并不表示免费。
