# Pi 模型能力与思考档位需求

> 状态：已实现并完成验收  
> 原则：预设完整可靠，自定义配置只采用能确认来源的值；外部目录不可用时不影响使用。

## 1. 当前版本边界

模型能力分为两条互不混用的路径：

- Pi 供应商预设由 CC Switch 维护完整模型配置，选择后可以直接保存。
- 自定义供应商从“获取模型列表”选中模型时，按 [Pi 前端设计指南 4.5](./pi-frontend-uiux-guidelines-zh.md) 的规则从同地址预设和 models.dev 补全 `reasoning`、`input`、`contextWindow`、`maxTokens`，并在协议允许时补 `thinkingLevelMap`（规则见第 4 节）。

预设里的思考映射仍只在开发时人工核对后写入。

这项功能不修改数据库 Schema，不参与显式供应商同步，也不管理 Pi 的默认供应商、默认模型、`auth.json`、路由、故障转移或插件。

## 2. 预设模型

`src/config/piModelCatalog.ts` 只用于复用 Pi 供应商预设中的模型知识，不作为自定义模型的全局识别器。

每个预设模型必须离线包含完整的 Pi 原生字段：

```ts
{
  id: string;
  name: string;
  reasoning: boolean;
  input: Array<"text" | "image">;
  contextWindow: number;
  maxTokens: number;
}
```

规则如下：

- 同一模型的公共能力在目录中维护一次。
- 供应商存在特殊限制时，由对应预设显式覆盖。
- 预设别名只在该预设中有效，不能让自定义供应商中的同名 ID 自动获得能力。
- 预设数据必须随应用发布，离线可用。
- GPT-5.6 Sol 的公共上下文长度为 `272000`。
- 不从其他应用的预设或 Pi 运行时模型列表动态生成 Pi 预设。

## 3. `thinkingLevelMap`

Pi 原生档位为：

```ts
type PiThinkingLevel =
  | "off"
  | "minimal"
  | "low"
  | "medium"
  | "high"
  | "xhigh"
  | "max";

type PiThinkingLevelMap = Partial<Record<PiThinkingLevel, string | null>>;
```

必须保留以下语义：

- 字符串表示发送给上游的实际值。
- `null` 表示该档位明确不可用。
- 缺少某个键表示该档位使用 Pi 默认行为。
- `{}` 表示整组明确使用 Pi 默认行为。
- 稀疏映射不得被自动补齐。
- `reasoning: true` 不代表所有档位都可用。

所有支持思考的预设模型都必须显式带有 `thinkingLevelMap`：

- 已确认接口语义的组合使用对应的映射。
- 尚无可靠专用映射的组合使用 `{}`，明确交给 Pi 原生行为。
- Anthropic 自适应思考等需要额外 Pi 原生兼容字段的预设，应同时写入必要的 `compat`，不能只让界面显示档位。

预设映射只在请求地址和协议都与预设相同时套用到自定义模型，并连同它依赖的 compat 一起补上（缺的才补；与用户已写的值冲突则不套用）。`openai-completions` 还要核对补完 compat 后的实际思考行为与预设一致，避免预设依赖 Pi 默认探测、用户又改了 compat 的情况。模型级 `baseUrl` 优先于供应商地址。其他供应商即使输入相同模型 ID，也不会套用预设映射。

## 4. 自定义模型

用户手动添加模型或从上游模型列表选择模型时：

- 模型 ID 写入表单。
- 显示名称在仍为空时同步为模型 ID，用户可以修改。
- 手动输入的模型 ID 不补全任何能力。从上游模型列表选中时，按设计指南 4.5 补全 `reasoning`、`input`、`contextWindow`、`maxTokens`，只补空字段、只往“支持”方向补，并提示用户核对。
- 选中的模型还没有 `thinkingLevelMap` 且支持扩展思考时补映射：同地址、同协议预设的映射优先，并补上它依赖的 compat；否则只在 Pi 会把档位原样作为 `reasoning_effort` 发出时（`openai-responses`，或探测为 OpenAI 思考格式且支持 `reasoning_effort` 的 `openai-completions`），按 Pi 官方 `getEffortThinkingLevelMap` 的规则，用 models.dev 的 effort 档位生成（先找按地址认出的供应商，认不出时取原厂条目）。DeepSeek、z.ai、OpenRouter、Moonshot 等有自己思考格式的地址，以及 Anthropic 等协议不生成。
- 上下文长度和最大输出 Token 必须是正数才能保存。
- “支持扩展思考”和“支持图片输入”默认分别为关闭和仅文本，用户可以随时修改。
- 开启扩展思考后才显示思考档位编辑器。
- 未填写 `thinkingLevelMap` 时由 Pi 使用原生默认档位；用户仍可在表单或配置 JSON 中写入字符串、`null`、稀疏映射或 `{}`。

界面不显示“自动值”“已覆盖自动值”或“恢复自动值”：补进来的值与手填值没有区别，不保留来源状态。

## 5. 配置 JSON 与旧配置

- 结构化字段和配置 JSON 双向同步。
- 用户已有的模型字段与 `thinkingLevelMap` 原样回显。
- 未知 JSON 字段无损保留。
- JSON 中的稀疏映射、单项 `null` 和 `{}` 不得改变语义。
- 旧模型缺少本版本要求的字段时，只在编辑表单中提示用户补全；用户保存后才落盘。
- 应用启动、供应商列表刷新和外部配置同步不得静默补写模型能力。

## 6. 验收

自动化测试至少覆盖：

- 每个预设模型都有完整的名称、思考能力、输入类型、上下文长度和最大输出 Token。
- 每个推理预设模型都显式拥有合法 `thinkingLevelMap`。
- 所有 GPT-5.6 Sol 预设均使用 `272000` 上下文。
- 手动输入模型 ID 不补全能力；从上游列表选中时只补空字段，已有思考映射不改，只在上述协议下按 Pi 官方规则生成映射，查不到时保持默认值。
- 自定义模型缺少名称、上下文长度或最大输出 Token 时不能保存，并聚焦错误字段。
- JSON 往返不丢未知字段或改变思考映射语义。

真实 Pi 验收使用隔离配置目录，并确认：

- 预设生成的完整原生模型配置可被 Pi 加载。
- Pi 对字符串、`null`、缺少键和 `{}` 的档位处理符合原生语义。
- 需要特殊兼容字段的预设产生正确的真实请求。
- 测试不修改用户实际的 `auth.json`、默认供应商或默认模型。

面向自定义供应商的模型数据库留待后续独立设计，不在本版本中提前保留运行时索引、网络回退或来源状态。
